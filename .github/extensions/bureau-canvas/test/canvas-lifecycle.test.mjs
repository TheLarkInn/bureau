import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import test from "node:test";

process.env.BUREAU_CANVAS_TEST = "1";
const canvas = await import("../extension.mjs");
const PAYLOAD = JSON.parse(await readFile(new URL("./fixtures/committed-payload.json", import.meta.url), "utf8"));
const READ_ACTIONS = new Set(["describe", "focus", "reload", "operations", "navigate"]);
const OPEN_REQUIRED = /Open the Bureau canvas/u;
const ROLE = { kind: "role", name: "retained-role", fields: {} };
const PIPELINE = "agent-eligible-pipeline";

function policySetting(t, value) {
  const previous = process.env.BUREAU_CANVAS_READ_ONLY;
  const set = (setting) => {
    if (setting === undefined) delete process.env.BUREAU_CANVAS_READ_ONLY;
    else process.env.BUREAU_CANVAS_READ_ONLY = setting;
  };
  set(value);
  t.after(() => set(previous));
}

async function fixture(t, registeredActions) {
  const dir = await mkdtemp(join(tmpdir(), "bureau-canvas-lifecycle-"));
  await mkdir(join(dir, "pipelines"));
  await writeFile(join(dir, "pipelines", `${PIPELINE}.yaml`),
    `name: ${PIPELINE}\nsteps:\n- name: verify\n  type: deterministic\n  run: printf before\n  next: done\n`);
  const runsDir = join(dir, "runs");
  await mkdir(runsDir);
  const instanceId = basename(dir);
  const registration = canvas.createBureauCanvasOptions(() => dir);
  const actions = registeredActions ?? registration.actions;
  const invoke = (name, input = {}) => actions.find((action) => action.name === name)
    .handler({ instanceId, input: { dir, ...input } });
  const close = () => registration.onClose({ instanceId });
  const open = (input = {}, options = {}) => canvas.openBureauCanvas(
    { instanceId, input: { dir, ...input } }, { payload: PAYLOAD, runsDir, ...options });
  t.after(async () => { await close(); await rm(dir, { recursive: true, force: true }); });
  return { dir, instanceId, actions, invoke, close, open };
}

async function refuseMutators(host, pattern = OPEN_REQUIRED) {
  for (const action of host.actions.filter(({ name }) => !READ_ACTIONS.has(name))) {
    await assert.rejects(host.invoke(action.name, ROLE), pattern, action.name);
  }
}

test("registered mutators refuse a never-opened instance rather than inferring local access", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  await refuseMutators(host);
  await host.open();
  assert.equal((await host.invoke("operations")).state.plan, null);
});

test("registered mutators refuse while an input-only read-only instance is still opening", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  const opening = host.open({ readOnly: true });
  try {
    await assert.rejects(host.invoke("create", ROLE), OPEN_REQUIRED);
  } finally {
    await opening;
  }
  await refuseMutators(host, /read-only/u);
  assert.equal((await host.invoke("operations")).state.plan, null);
});

test("registered mutators refuse after listening fails and cannot downgrade a failed read-only open", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  await assert.rejects(host.open({ readOnly: true }, { port: -1 }), { code: "ERR_SOCKET_BAD_PORT" });
  assert.equal(canvas.servers.has(host.instanceId), false);
  await refuseMutators(host);
  await host.open({ readOnly: false });
  await refuseMutators(host, /read-only/u);
});

test("a read-only reopen restricts the existing instance before asynchronous refresh completes", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  await host.open();
  const reopening = host.open({ readOnly: true });
  try {
    await refuseMutators(host, /read-only/u);
  } finally {
    await reopening;
  }
  assert.equal((await host.invoke("operations")).state.plan, null);
});

test("registered mutators refuse incomplete context even when a server record exists", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  await host.open();
  const entry = canvas.servers.get(host.instanceId);
  const access = entry.access;
  try {
    entry.initialized = false;
    await refuseMutators(host);
    entry.initialized = true;
    for (const policy of [undefined, {}, { mode: "unknown" }]) {
      entry.access = policy;
      await refuseMutators(host);
    }
  } finally {
    entry.initialized = true;
    entry.access = access;
  }
  assert.equal((await host.invoke("operations")).state.plan, null);
});

test("a stopped listener cannot authorize registered mutators through its remaining record", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  await host.open();
  const entry = canvas.servers.get(host.instanceId);
  await new Promise((resolve) => entry.server.close(resolve));
  await refuseMutators(host);
});

test("input-only read-only restrictions survive close and repeated downgrade attempts", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  await host.open({ readOnly: true });
  await host.close();
  await refuseMutators(host);
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await host.open({ readOnly: false }, { readOnly: false, access: { mode: "local" } });
    assert.equal((await host.invoke("operations")).state.access.mode, "read-only");
    await refuseMutators(host, /read-only/u);
    await host.close();
  }
});

test("invalid host settings remain fail-closed after correction, close, and reopen", async (t) => {
  policySetting(t, "invalid-setting");
  const host = await fixture(t);
  await host.open();
  await host.close();
  process.env.BUREAU_CANVAS_READ_ONLY = "0";
  await refuseMutators(host);
  await host.open({ readOnly: false });
  const result = await host.invoke("operations");
  assert.match(result.state.access.reason, /^Invalid BUREAU_CANVAS_READ_ONLY/u);
  await refuseMutators(host, /read-only/u);
});

test("closed-instance refusals preserve CRUD plans for an authorized local reopen", async (t) => {
  policySetting(t, undefined);
  const host = await fixture(t);
  await host.open();
  await host.invoke("create", ROLE);
  const before = (await host.invoke("operations", { refresh: true })).state.plan;
  assert.equal(before.writes.length, 1);
  await host.close();
  await refuseMutators(host);
  await host.open();
  assert.deepEqual((await host.invoke("operations")).state.plan, before);
  await host.invoke("create", { ...ROLE, name: "another-role" });
  assert.equal((await host.invoke("operations", { refresh: true })).state.plan.writes.length, 2);
});

test("a retained field draft saves only after a valid local reopen, without force", async (t) => {
  policySetting(t, undefined);
  let writes = 0;
  const actions = canvas.canvasActions({
    loadFindings: async (dir) => ({ ...PAYLOAD, dir, state: "validated", ok: true }),
    validateDraft: async () => ({ state: "validated", ok: true, findings: [] }),
    writeText: async (...args) => { writes += 1; await writeFile(...args); },
  });
  const host = await fixture(t, actions);
  const path = join(host.dir, "pipelines", `${PIPELINE}.yaml`);
  const before = await readFile(path, "utf8");
  await host.open();
  await host.invoke("set_field", { pipeline: PIPELINE, step: "verify", field: "run", value: "printf lifecycle-check" });
  await host.close();
  await assert.rejects(host.invoke("save", { pipeline: PIPELINE }), OPEN_REQUIRED);
  assert.deepEqual([writes, await readFile(path, "utf8")], [0, before]);
  await host.open();
  const saved = await host.invoke("save", { pipeline: PIPELINE });
  assert.deepEqual([saved.saved, saved.forced, writes], [true, false, 1]);
  assert.match(await readFile(path, "utf8"), /printf lifecycle-check/u);
});
