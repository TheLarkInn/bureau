import assert from "node:assert/strict";
import { readFile, stat, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { actions } from "../lib/actions.mjs";
import { parse } from "../lib/codec.mjs";
import { findings } from "../lib/findings.mjs";

const stub = fileURLToPath(new URL("./fixtures/actions-bureau.mjs", import.meta.url));

function fixtureDir(name) {
  return fileURLToPath(new URL(`./fixtures/${name}/.bureau`, import.meta.url));
}

function action(name) {
  return actions.find((candidate) => candidate.name === name);
}

function depsFor(name, extra = {}) {
  const draftStore = new Map();
  return {
    getSubject: () => ({ dir: fixtureDir(name) }),
    getDraft: (instanceId) => draftStore.get(instanceId),
    setDraft: (instanceId, draft) => draftStore.set(instanceId, draft),
    clearDraft: (instanceId) => draftStore.delete(instanceId),
    loadFindings: (dir) => findings(dir, { binary: stub }),
    ...(name === "actions-edit" ? { validateDraft: validateEditDraft } : {}),
    draftStore,
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
      ["set_field", false],
      ["rewire", false],
      ["save", false],
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

test("set_field updates a draft and publishes without saving", async () => {
  await withPipelineFile(async (original) => {
    const events = [];
    const deps = depsFor("actions-edit", { publish: async (instanceId, event, payload) => events.push({ instanceId, event, payload }) });
    const result = await action("set_field").handler(
      { instanceId: "actions-set-field", input: { pipeline: "agent-eligible-pipeline", step: "verify", field: "run", value: "cargo test -p bureau" } },
      deps,
    );

    assert.deepEqual(
      { run: step(result, "verify").fields.run, events: events.length, saved: await pipelineText() },
      { run: "cargo test -p bureau", events: 1, saved: original },
    );
  });
});

test("set_field illegal edit is rejected with the CLI message", async () => {
  await withPipelineFile(async () => {
    const message = await rejectsWithMessage(() =>
      action("set_field").handler(
        { instanceId: "actions-illegal-field", input: { pipeline: "agent-eligible-pipeline", step: "verify", field: "role", value: "implementer" } },
        depsFor("actions-edit"),
      ),
    );

    assert.equal(message, "pipeline `agent-eligible-pipeline` step `verify`: `role` does not apply to deterministic steps");
  });
});

test("rewire moves an edge in memory before save", async () => {
  await withPipelineFile(async (original) => {
    const result = await action("rewire").handler(
      { instanceId: "actions-rewire", input: { pipeline: "agent-eligible-pipeline", step: "verify", outcome: "failure", target: "done" } },
      depsFor("actions-edit"),
    );

    assert.deepEqual(
      { target: edge(result, "verify", "failure").target, saved: await pipelineText() },
      { target: "terminal:done", saved: original },
    );
  });
});

test("valid rewire plus save changes one line and preserves CRLF", async () => {
  await withPipelineFile(async (original) => {
    const crlf = original.replaceAll("\r\n", "\n").replaceAll("\n", "\r\n");
    await writePipelineText(crlf);
    const deps = depsFor("actions-edit");
    await action("rewire").handler(
      { instanceId: "actions-save-rewire", input: { pipeline: "agent-eligible-pipeline", step: "verify", outcome: "failure", target: "done" } },
      deps,
    );
    await action("save").handler({ instanceId: "actions-save-rewire", input: { pipeline: "agent-eligible-pipeline" } }, deps);
    const saved = await pipelineText();

    assert.deepEqual(
      { changes: changedLines(crlf, saved), hasCrLf: saved.includes("\r\n") },
      { changes: [[13, "  on_failure: escalate", "  on_failure: done"]], hasCrLf: true },
    );
  });
});

test("deleting an outcome branch removes the key instead of writing abort", async () => {
  await withPipelineFile(async () => {
    const deps = depsFor("actions-edit");
    await action("rewire").handler(
      { instanceId: "actions-delete-branch", input: { pipeline: "agent-eligible-pipeline", step: "verify", outcome: "failure", target: null } },
      deps,
    );
    await action("save").handler({ instanceId: "actions-delete-branch", input: { pipeline: "agent-eligible-pipeline" } }, deps);
    const saved = await pipelineText();

    assert.deepEqual([verifyBlock(saved).includes("  on_failure:"), saved.includes("  on_failure: abort")], [false, false]);
  });
});

test("save refuses invalid drafts unless forced", async () => {
  await withPipelineFile(async () => {
    const deps = depsFor("actions-edit");
    const draft = await invalidDraft("actions-edit", "missing");
    deps.setDraft("actions-force-save", draft);
    const message = await rejectsWithMessage(() =>
      action("save").handler({ instanceId: "actions-force-save", input: { pipeline: "agent-eligible-pipeline" } }, deps),
    );
    deps.setDraft("actions-force-save", draft);
    const result = await action("save").handler({ instanceId: "actions-force-save", input: { pipeline: "agent-eligible-pipeline", force: true } }, deps);

    assert.deepEqual(
      { message, forced: result.forced, saved: (await pipelineText()).includes("next: missing") },
      { message: "pipeline `agent-eligible-pipeline` step `verify`: unknown next target `missing`", forced: true, saved: true },
    );
  });
});

test("reload discards an unsaved edit", async () => {
  await withPipelineFile(async () => {
    const deps = depsFor("actions-edit");
    await action("rewire").handler(
      { instanceId: "actions-reload-discard", input: { pipeline: "agent-eligible-pipeline", step: "verify", outcome: "failure", target: "done" } },
      deps,
    );
    await action("reload").handler({ instanceId: "actions-reload-discard", input: { pipeline: "agent-eligible-pipeline" } }, deps);
    const result = await action("describe").handler({ instanceId: "actions-reload-discard", input: { pipeline: "agent-eligible-pipeline" } }, deps);

    assert.equal(edge(result, "verify", "failure").target, "terminal:escalate");
  });
});

test("scratch validation runs outside the config tree and is removed", async () => {
  await withPipelineFile(async () => {
    const seen = [];
    const deps = depsFor("actions-edit", {
      validateDraft: undefined,
      loadFindings: async (dir) => {
        seen.push(dir);
        return { ok: true, state: "validated", dir, errors: [], findings: [], config: {} };
      },
    });
    await action("set_field").handler(
      { instanceId: "actions-scratch", input: { pipeline: "agent-eligible-pipeline", step: "verify", field: "run", value: "cargo test" } },
      deps,
    );

    assert.deepEqual(
      { count: seen.length, inConfigTree: seen[0].startsWith(fixtureDir("actions-edit")), stillExists: await exists(seen[0]) },
      { count: 1, inConfigTree: false, stillExists: false },
    );
  });
});

test("set_field edits a role agent through the draft save path", async () => {
  await withRoleFile("plugins-edit", async (original) => {
    const deps = depsFor("plugins-edit");
    await action("set_field").handler({ instanceId: "plugins-role", input: { role: "implementer", field: "agent", value: "/bureau:reviewer" } }, deps);
    await action("save").handler({ instanceId: "plugins-role", input: { role: "implementer" } }, deps);

    assert.deepEqual(
      { changed: changedLines(original, await roleText("plugins-edit", "implementer")) },
      { changed: [[2, "agent: /bureau:implementer", "agent: /bureau:reviewer"]] },
    );
  });
});

test("advisories do not block save", async () => {
  await withPipelineFile(async () => {
    const deps = depsFor("actions-edit", {
      loadAdvisories: async () => [{ source: "advisory", marker: "script-advisory", message: "missing", path: "pipelines/agent-eligible-pipeline.yaml" }],
    });
    await action("set_field").handler(
      { instanceId: "actions-advisory-save", input: { pipeline: "agent-eligible-pipeline", step: "verify", field: "run", value: "./missing.sh" } },
      deps,
    );
    const result = await action("save").handler({ instanceId: "actions-advisory-save", input: { pipeline: "agent-eligible-pipeline" } }, deps);

    assert.deepEqual({ saved: result.saved, markers: result.findings.map((item) => item.marker) }, { saved: true, markers: ["script-advisory"] });
  });
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

async function withPipelineFile(run) {
  const original = await pipelineText();
  try {
    await run(original);
  } finally {
    await writePipelineText(original);
  }
}

async function withRoleFile(name, run) {
  const original = await roleText(name, "implementer");
  try {
    await run(original);
  } finally {
    await writeRoleText(name, "implementer", original);
  }
}

async function invalidDraft(name, target) {
  const path = fileURLToPath(new URL(`./fixtures/${name}/.bureau/pipelines/agent-eligible-pipeline.yaml`, import.meta.url));
  const text = await readFile(path, "utf8");
  const parsed = parse(text, { path: "pipelines/agent-eligible-pipeline.yaml" });
  edge(parsed, "verify", "success").target = target;
  return { subject: { dir: fixtureDir(name), pipeline: "agent-eligible-pipeline" }, path, ...parsed };
}

async function rejectsWithMessage(call) {
  try {
    await call();
  } catch (error) {
    return error.message;
  }
  return "did not reject";
}

function step(result, name) {
  return result.view.steps.find((item) => item.name === name);
}

function edge(result, source, outcome) {
  return result.view.edges.find((item) => item.source === source && item.outcome === outcome);
}

async function pipelineText() {
  return readFile(new URL("./fixtures/actions-edit/.bureau/pipelines/agent-eligible-pipeline.yaml", import.meta.url), "utf8");
}

async function roleText(name, role) {
  return readFile(new URL(`./fixtures/${name}/.bureau/roles/${role}.yaml`, import.meta.url), "utf8");
}

async function writeRoleText(name, role, text) {
  await writeFile(new URL(`./fixtures/${name}/.bureau/roles/${role}.yaml`, import.meta.url), text);
}

async function writePipelineText(text) {
  await writeFile(new URL("./fixtures/actions-edit/.bureau/pipelines/agent-eligible-pipeline.yaml", import.meta.url), text);
}

async function exists(path) {
  return stat(path).then(
    () => true,
    () => false,
  );
}

function changedLines(before, after) {
  const left = before.split(/\r?\n/u);
  const right = after.split(/\r?\n/u);
  const changes = [];
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    if (left[index] !== right[index]) {
      changes.push([index + 1, left[index], right[index]]);
    }
  }
  return changes;
}

function verifyBlock(text) {
  const start = text.indexOf("- name: verify");
  const end = text.indexOf("\n- ", start + 1);
  return text.slice(start, end === -1 ? undefined : end);
}

function validateEditDraft(draft) {
  const errors = [...roleErrors(draft), ...targetErrors(draft)];
  return {
    ok: errors.length === 0,
    state: "validated",
    dir: draft.subject.dir,
    errors,
    config: errors.length === 0 ? {} : null,
    findings: errors.map((error) => ({ source: "bureau-validate", marker: "validation", ...error })),
  };
}

function roleErrors(draft) {
  return step({ view: draft.view }, "verify").fields.role
    ? [{ path: "pipelines/agent-eligible-pipeline.yaml", message: "pipeline `agent-eligible-pipeline` step `verify`: `role` does not apply to deterministic steps" }]
    : [];
}

function targetErrors(draft) {
  const targets = new Set(["done", "abort", "escalate", ...draft.view.steps.map((item) => item.name)]);
  return draft.view.edges.flatMap((item) =>
    item.relation === "control" && !targets.has(item.target.replace("terminal:", ""))
      ? [{ path: "pipelines/agent-eligible-pipeline.yaml", message: `pipeline \`agent-eligible-pipeline\` step \`${item.source}\`: unknown next target \`${item.target}\`` }]
      : [],
  );
}
