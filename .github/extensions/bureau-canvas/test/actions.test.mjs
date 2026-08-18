import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { actions } from "../lib/actions.mjs";
import { findings } from "../lib/findings.mjs";

const stub = fileURLToPath(new URL("./fixtures/actions-bureau.mjs", import.meta.url));

function fixtureDir(name) {
  return fileURLToPath(new URL(`./fixtures/${name}/.bureau`, import.meta.url));
}

function action(name) {
  return actions.find((candidate) => candidate.name === name);
}

function depsFor(name, extra = {}) {
  return {
    getSubject: () => ({ dir: fixtureDir(name) }),
    loadFindings: (dir) => findings(dir, { binary: stub }),
    ...extra,
  };
}

test("declared action names avoid the reserved prefix", () => {
  assert.deepEqual(
    actions.map((candidate) => [candidate.name, candidate.name.startsWith("canvas.")]),
    [
      ["describe", false],
      ["focus", false],
      ["reload", false],
    ],
  );
});

test("describe returns config view with findings and orphans", async () => {
  const result = await action("describe").handler({ instanceId: "actions-config", input: {} }, depsFor("actions-valid"));

  assert.deepEqual(
    {
      scope: result.scope,
      ok: result.ok,
      findings: result.findings.length,
      orphans: result.view.orphans,
    },
    {
      scope: "config",
      ok: true,
      findings: 0,
      orphans: [{ kind: "role", name: "unused" }],
    },
  );
});

test("describe pipeline returns each edge relation", async () => {
  const result = await action("describe").handler(
    { instanceId: "actions-pipeline", input: { pipeline: "agent-eligible-pipeline" } },
    depsFor("actions-valid"),
  );
  const relations = [...new Set(result.view.edges.map((edge) => edge.relation))].sort();

  assert.deepEqual(
    { scope: result.scope, pipeline: result.subject.pipeline, relations },
    { scope: "pipeline", pipeline: "agent-eligible-pipeline", relations: ["control", "data", "observes"] },
  );
});

test("focus publishes once without reading disk", async () => {
  const events = [];
  let reads = 0;
  const deps = depsFor("actions-valid", {
    loadFindings: async () => {
      reads += 1;
      return {};
    },
    publish: async (instanceId, event, payload) => events.push({ instanceId, event, payload }),
  });

  const result = await action("focus").handler({ instanceId: "actions-focus", input: { kind: "role", name: "implementer" } }, deps);

  assert.deepEqual({ result, events, reads }, {
    result: {
      focus: { kind: "role", name: "implementer" },
      subject: { dir: fixtureDir("actions-valid") },
    },
    events: [
      {
        instanceId: "actions-focus",
        event: "focus",
        payload: {
          focus: { kind: "role", name: "implementer" },
          subject: { dir: fixtureDir("actions-valid") },
        },
      },
    ],
    reads: 0,
  });
});

test("reload reflects a disk change", async () => {
  await withChangingState("actions-change", { agent: "/bureau:first", valid: true }, async () => {
    const deps = depsFor("actions-change");
    const before = await action("reload").handler({ instanceId: "actions-reload", input: {} }, deps);
    await writeState("actions-change", { agent: "/bureau:second", valid: true });
    const after = await action("reload").handler({ instanceId: "actions-reload", input: {} }, deps);

    assert.deepEqual([before.view.roles[0].agent, after.view.roles[0].agent], ["/bureau:first", "/bureau:second"]);
  });
});

test("reload returns errors after config becomes invalid", async () => {
  await withChangingState("actions-invalidating", { agent: "/bureau:first", valid: true }, async () => {
    const deps = depsFor("actions-invalidating");
    await action("reload").handler({ instanceId: "actions-invalid", input: {} }, deps);
    await writeState("actions-invalidating", { agent: "/bureau:first", valid: false });
    const result = await action("reload").handler({ instanceId: "actions-invalid", input: {} }, deps);

    assert.deepEqual(
      { ok: result.ok, state: result.state, errors: result.errors.length, findings: result.findings.length, repos: result.view.repos },
      { ok: false, state: "validated", errors: 1, findings: 1, repos: [] },
    );
  });
});

test("each action declares an input schema", () => {
  assert.equal(actions.every((candidate) => candidate.inputSchema?.type === "object"), true);
});

async function withChangingState(name, state, run) {
  const url = stateUrl(name);
  const original = await readFile(url, "utf8");
  try {
    await writeState(name, state);
    await run();
  } finally {
    await writeFile(url, original);
  }
}

async function writeState(name, state) {
  await writeFile(stateUrl(name), `${JSON.stringify(state, null, 2)}\n`);
}

function stateUrl(name) {
  return new URL(`./fixtures/${name}/.bureau/actions-state.json`, import.meta.url);
}
