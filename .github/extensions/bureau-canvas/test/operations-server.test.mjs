import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { operationsOverview } from "../web/operations.mjs";
import { readHistory } from "../web/live/history.mjs";

process.env.BUREAU_CANVAS_TEST = "1";
const canvas = await import("../extension.mjs");
const PAYLOAD = JSON.parse(await readFile(new URL("./fixtures/committed-payload.json", import.meta.url), "utf8"));
const PIPELINE = "agent-eligible-pipeline";

async function host(t, options = {}) {
  const dir = await mkdtemp(join(tmpdir(), "bureau-ops-server-"));
  const runsDir = join(dir, "runs");
  await mkdir(runsDir);
  const instanceId = `operations-${dir.split(/[\\/]/u).at(-1)}`;
  const calls = [];
  const opened = await canvas.openBureauCanvas({ instanceId, input: { dir } }, {
    payload: PAYLOAD, runsDir, exec: (args) => { calls.push(args); return null; }, ...options,
  });
  t.after(async () => { await canvas.closeBureauCanvas({ instanceId }); await rm(dir, { recursive: true, force: true }); });
  const entry = canvas.servers.get(instanceId);
  const post = async (body) => (await fetch(new URL("/intent", opened.url), {
    method: "POST", headers: { "Content-Type": "application/json", "X-Bureau-Capability": entry.capability },
    body: JSON.stringify(body),
  })).json();
  const action = (name, input = {}) => canvas.canvasActions().find((item) => item.name === name).handler({ instanceId, input });
  return { dir, runsDir, instanceId, opened, post, action, calls };
}

async function seed(host, id = "observed-run", tail = []) {
  await mkdir(join(host.runsDir, id));
  const events = [{ kind: "run_started", data: { run_id: id, assignment: "agent-eligible",
    snapshot: { pipeline: { name: PIPELINE } } } }, ...tail]
    .map((event, seq) => ({ seq, at_ms: Date.now() - 1000 + seq, ...event }));
  await writeFile(join(host.runsDir, id, "events.jsonl"), `${events.map((event) => JSON.stringify(event)).join("\n")}\n`);
}

test("default Operations and host/iframe actions share the actual projection without executing work", async (t) => {
  const server = await host(t);
  await seed(server);
  const result = await server.action("operations");
  const browser = await server.post({ kind: "operations" });
  assert.deepEqual([result.state.navigation.view, result.overview.counts, result.overview, server.calls],
    ["operations", { attention: 0, active: 1, paused: 0, failed: 0 },
      operationsOverview(result.state, result.listing), []]);
  assert.deepEqual(browser.overview.counts, result.overview.counts);
});

test("host navigation and iframe navigation select the same exact run and preserve it through refresh", async (t) => {
  const server = await host(t);
  await seed(server);
  const input = { view: "pipeline", pipeline: PIPELINE, mode: "live", run_id: "observed-run" };
  const app = await server.action("navigate", input);
  const browser = await server.post({ kind: "navigate", input });
  const refreshed = await server.action("operations", { refresh: true });
  const selection = (state) => ({ ...state.navigation, revision: undefined });
  assert.deepEqual([selection(app), selection(browser.state), selection(refreshed.state)], Array(3).fill({ ...input, revision: undefined }));
  assert.deepEqual(server.calls, []);
});

test("unknown or misattributed run navigation fails without changing the selection", async (t) => {
  const server = await host(t);
  const input = { view: "pipeline", pipeline: PIPELINE, mode: "live", run_id: "not-present" };
  await assert.rejects(server.action("navigate", input), /not attributed/u);
  const browser = await server.post({ kind: "navigate", input });
  assert.deepEqual([browser.ok, (await server.action("operations")).state.navigation.view], [false, "operations"]);
});

test("refreshing config revalidates saved files without discarding a pending plan", async (t) => {
  const server = await host(t);
  const created = await server.post({ kind: "create", input: { kind: "role", name: "extra-review", fields: {} } });
  assert.equal(created.ok, true);
  const before = created.state.plan;
  const refreshed = await server.post({ kind: "operations", refresh: true });
  assert.deepEqual([refreshed.ok, refreshed.state.plan, refreshed.overview.configuration.pending], [true, before, 1]);
});

test("an in-flight authoring refresh does not overwrite a newer navigation request", async (t) => {
  const server = await host(t);
  const refresh = server.action("operations", { refresh: true });
  const chosen = await server.action("navigate", { view: "pipeline", pipeline: PIPELINE, mode: "replay" });
  const refreshed = await refresh;
  assert.deepEqual([refreshed.state.navigation, refreshed.state.selectedPipeline.name],
    [chosen.navigation, PIPELINE]);
});

test("app and iframe reject malformed navigation fields without changing the view", async (t) => {
  const server = await host(t);
  for (const input of [{ view: "config", assignment: false }, { view: "pipeline", pipeline: PIPELINE, mode: null }]) {
    await assert.rejects(server.action("navigate", input), /nonempty strings/u);
    assert.equal((await server.post({ kind: "navigate", input })).ok, false);
  }
  assert.equal((await server.action("operations")).state.navigation.view, "operations");
});

test("invalid empty configuration stays invalid in the actual host overview", async (t) => {
  const server = await host(t, { payload: { ok: false, config: null, errors: ["assignments/work.yaml: invalid YAML"] } });
  const result = await server.action("operations");
  assert.deepEqual([result.overview.configuration.verdict, result.overview.configuration.errors, result.overview.assignments],
    ["Invalid configuration", ["assignments/work.yaml: invalid YAML"], []]);
});

test("corrupt logs remain visible, and Replay cannot silently turn their prefix into success", async (t) => {
  const server = await host(t);
  await seed(server, "corrupt");
  await writeFile(join(server.runsDir, "corrupt", "events.jsonl"), "not-json\n");
  const result = await server.action("operations");
  const response = await fetch(new URL("/runs/corrupt/events", server.opened.url));
  assert.deepEqual([result.overview.runs[0].label, result.overview.runs[0].target, result.overview.runs[0].cost, response.status],
    ["Evidence unavailable", null, "Unknown", 422]);
  await assert.rejects(readHistory(response), /Invalid JSON/u);
});

test("an incompatible CLI cannot fabricate empty successful run history", async (t) => {
  const server = await host(t, { exec: () => ({ code: 0, stdout: "not JSON", stderr: "" }) });
  const response = await fetch(new URL("/runs/example/events", server.opened.url));
  assert.equal(response.status, 422);
  await assert.rejects(readHistory(response), /no event array/u);
});

test("CLI and raw history preserve supported sequence gaps and duplicates, including a valid final unframed record", async (t) => {
  const events = [
    { seq: 5, kind: "run_started", data: { run_id: "observed-run", assignment: "agent-eligible" } },
    { seq: 999, kind: "step_started", data: { step: "implement" } },
    { seq: 999, kind: "step_started", data: { step: "implement" } },
    { seq: 2, kind: "run_finished", data: { outcome: "success" } },
  ].map((event) => ({ ...event, at_ms: Date.now() - 1000 }));
  for (const exec of [() => null, () => ({ code: 0, stdout: JSON.stringify(events), stderr: "" })]) {
    const server = await host(t, { exec });
    await seed(server);
    await writeFile(join(server.runsDir, "observed-run", "events.jsonl"), events.map((event) => JSON.stringify(event)).join("\n"));
    const response = await fetch(new URL("/runs/observed-run/events", server.opened.url));
    const history = await readHistory(response);
    const overview = (await server.action("operations")).overview;
    assert.deepEqual([history.events.map((event) => event.seq), overview.runs[0].status, overview.runs[0].target?.mode],
      [[5, 999, 999, 2], "completed", "replay"]);
  }
});

test("torn raw history advertises its readable prefix rather than hiding uncertainty", async (t) => {
  const server = await host(t);
  await seed(server);
  const path = join(server.runsDir, "observed-run", "events.jsonl");
  await writeFile(path, `${await readFile(path, "utf8")}{"seq":999`);
  const history = await readHistory(await fetch(new URL("/runs/observed-run/events", server.opened.url)));
  assert.deepEqual([history.events.length, Boolean(history.warning), (await server.action("operations")).overview.runs[0].needs_attention],
    [1, true, true]);
});

test("read-only host rejects every mutation category before CLI dispatch or config planning", async (t) => {
  const server = await host(t, { readOnly: true });
  const mutations = ["reconcile-now", "pause-run", "resume-run", "cancel-run", "retry", "clone",
    "create", "delete", "rename", "save-plan", "discard-plan", "save-pipeline", "set-work-source",
    "set-assignment-runtime", "set-repos", "set-limits", "unknown-mutation"];
  for (const kind of mutations) {
    const response = await server.post({ kind, run_id: "observed-run", readOnly: false, access: { mode: "local" },
      input: { kind: "role", name: "must-not-be-created", fields: {} } });
    assert.deepEqual([response.ok, response.access.mode, /read-only/u.test(response.error)], [false, "read-only", true]);
  }
  for (const action of canvas.canvasActions().filter(({ name }) => !["describe", "focus", "reload", "operations", "navigate"].includes(name))) {
    await assert.rejects(server.action(action.name, { readOnly: false }), /read-only/u);
  }
  const result = await server.action("operations");
  assert.deepEqual([server.calls, result.state.plan, result.overview.access.mode], [[], null, "read-only"]);
});

test("read-only policy survives navigation, refresh, forged fields, and reopening the same canvas", async (t) => {
  const server = await host(t, { readOnly: true });
  await server.action("navigate", { view: "pipeline", pipeline: PIPELINE, mode: "live" });
  await server.post({ kind: "operations", refresh: true, access: { mode: "local" }, readOnly: false });
  await canvas.openBureauCanvas({ instanceId: server.instanceId, input: { dir: server.dir, readOnly: false } },
    { payload: PAYLOAD, readOnly: false, access: { mode: "local" }, runsDir: server.runsDir });
  assert.equal((await server.action("operations")).state.access.mode, "read-only");
  await assert.rejects(server.action("save"), /read-only/u);
});

test("invalid host policy remains read-only after its environment changes", async (t) => {
  const previous = process.env.BUREAU_CANVAS_READ_ONLY;
  const restore = () => {
    if (previous === undefined) delete process.env.BUREAU_CANVAS_READ_ONLY;
    else process.env.BUREAU_CANVAS_READ_ONLY = previous;
  };
  t.after(restore);
  process.env.BUREAU_CANVAS_READ_ONLY = "invalid-setting";
  const server = await host(t, { readOnly: false });
  restore();
  const result = await server.action("operations", { refresh: true });
  assert.deepEqual([result.overview.access.mode, result.overview.access.reason.startsWith("Invalid BUREAU_CANVAS_READ_ONLY"), server.calls],
    ["read-only", true, []]);
  assert.equal((await server.post({ kind: "reconcile-now", readOnly: false })).ok, false);
});

test("read-only history never invokes a potentially repairing CLI or native control probe", async (t) => {
  const server = await host(t, { readOnly: true });
  await seed(server);
  const path = join(server.runsDir, "observed-run", "events.jsonl");
  const text = `${await readFile(path, "utf8")}{"seq":`;
  await writeFile(path, text);
  const history = await readHistory(await fetch(new URL("/runs/observed-run/events", server.opened.url)));
  const controls = await fetch(new URL("/runs/observed-run/controls", server.opened.url));
  assert.deepEqual([history.source, Boolean(history.warning), controls.status, server.calls, await readFile(path, "utf8")],
    ["log", true, 403, [], text]);
});
