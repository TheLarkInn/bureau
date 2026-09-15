import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";

import { actions } from "../lib/actions.mjs";
import { createDocument, parse, parseValue, render } from "../lib/codec.mjs";
import { editable, removeStep, renameStep, setStepField } from "../lib/edit.mjs";
import { savePipeline } from "../lib/pipeline.mjs";
import { scaffoldStep } from "../lib/steps.mjs";
import { pipelineView } from "../lib/view.mjs";
import { FACTORY_PROFILE, factoryProblems, factorySchema, newFactory, stepFactoryProblems } from "../web/copilot-factory.mjs";

const FACTORY = JSON.parse(await readFile(new URL("./fixtures/copilot-factory.json", import.meta.url), "utf8"));
const PIPELINE = {
  name: "factory",
  steps: [
    { name: "verify", type: "deterministic", run: "true", next: "review" },
    { name: "review", type: "agent", role: "reviewer", copilot_factory: FACTORY, inputs_from: ["verify"], next: "done" },
  ],
};
const SOURCE = `# Reviewed executable authority\n${createDocument(PIPELINE)}`;
const PATH = "pipelines/factory.yaml";
const ROLES = [{ name: "reviewer", adapter: "copilot", permissions: ["model:invoke"] }];

test("factory schema is closed without restricting opaque argument keys", () => {
  assert.deepEqual([
    factorySchema.additionalProperties,
    factorySchema.properties.runtime.additionalProperties,
    factorySchema.properties.limits.additionalProperties,
    factorySchema.properties.args.type,
    factoryProblems(FACTORY),
  ], [false, false, false, ["object", "null"], []]);
});

test("factory validation rejects malformed authority, paths, args and ceilings", () => {
  const cases = [
    [], null, { ...FACTORY, extra: true }, { ...FACTORY, name: "not a name" },
    { ...FACTORY, extension: "user:review" }, { ...FACTORY, extension: "project:../review" },
    { ...FACTORY, extension_digest: "a".repeat(64) }, { ...FACTORY, metadata: "../factory.json" },
    ...[null, "", " ", "../model", "env:MODEL", "model.name", [], 1]
      .map((model_credential) => ({ ...FACTORY, model_credential })),
    ...["{}", [], false, 1].map((args) => ({ ...FACTORY, args })),
    ...[null, [], { max_ai_credits: null }, { timeout_seconds: 0 }, { max_total_subagents: 1.5 },
      { max_ai_credits: Infinity }, { typo: 1 }].map((limits) => ({ ...FACTORY, limits })),
    ...[null, [], { ...FACTORY.runtime, profile: "3" }, { ...FACTORY.runtime, directory: "relative" },
      { ...FACTORY.runtime, executable: "/usr/bin/node" }, { ...FACTORY.runtime, dist: "../dist" },
      { ...FACTORY.runtime, cli: "..\\entry.mjs" }, { ...FACTORY.runtime, version: " " },
      { ...FACTORY.runtime, arbitraryArgs: [] }].map((runtime) => ({ ...FACTORY, runtime })),
  ];
  for (const value of cases) {
    assert.ok(factoryProblems(value).length > 0, JSON.stringify(value));
  }
});

test("model authentication requires a declared reference and preserves it across snapshots and clones", () => {
  const missing = structuredClone(FACTORY);
  delete missing.model_credential;
  assert.ok(factorySchema.required.includes("model_credential"));
  assert.ok(factoryProblems(missing).some((message) => message.includes("model_credential")));
  const parsed = parse(SOURCE, { path: PATH });
  const clone = scaffoldStep("agent", "copy", { role: "reviewer", copilotFactory: FACTORY });
  assert.deepEqual([parsed.view.steps[1].fields.copilotFactory.model_credential,
    clone.fields.copilotFactory.model_credential], ["copilot-model", "copilot-model"]);
});

test("omitted static args and optional ceilings remain valid, without fabricated budgets", () => {
  const value = structuredClone(FACTORY);
  delete value.args;
  delete value.limits;
  delete value.metadata;
  value.runtime.dist = ".";
  value.runtime.cli = null;
  assert.deepEqual(factoryProblems(value), []);
});

test("the shared schema and hints enforce the supported SDK ceilings", () => {
  const limits = { max_concurrent_subagents: 500, max_total_subagents: 4294967295, timeout_seconds: 3000000 };
  const action = actions.find((action) => action.name === "set_copilot_factory");
  assert.deepEqual([
    factoryProblems({ ...FACTORY, limits }),
    action.inputSchema.properties.factory.anyOf[0].properties.limits.properties.max_concurrent_subagents.maximum,
    factoryProblems({ ...FACTORY, limits: { max_concurrent_subagents: 501 } }),
  ], [[], 500, ["limits.max_concurrent_subagents must be a positive integer no greater than 500; null does not remove a ceiling"]]);
});

test("the Bureau SDK capability profile is exact across declarations, defaults and validation", () => {
  assert.deepEqual([FACTORY_PROFILE, FACTORY.runtime.profile, newFactory().runtime.profile,
    factorySchema.properties.runtime.properties.profile.const],
  Array(4).fill("copilot-sdk-factory-v1"));
  for (const profile of ["COPILOT-SDK-FACTORY-V1", "copilot-sdk-factory-v2", "copilot-sdk-factory-v1 ", "1.0.0", "protocol-3"]) {
    assert.deepEqual(factoryProblems({ ...FACTORY, runtime: { ...FACTORY.runtime, profile } }),
      ["runtime.profile must be Bureau's SDK capability contract copilot-sdk-factory-v1"]);
  }
});

test("codec and config snapshots preserve the whole factory declaration", () => {
  const parsed = parse(SOURCE, { path: PATH });
  const projected = pipelineView({ config: { pipelines: { factory: PIPELINE } } }, "factory");
  assert.deepEqual([
    parsed.view.steps[1].fields.copilotFactory,
    projected.steps[1].fields.copilotFactory,
    render(parsed.view, parsed.doc, parsed.style),
  ], [FACTORY, FACTORY, SOURCE]);
});

test("factory edits and explicit removal round-trip without replacing other fields", () => {
  const parsed = parse(SOURCE, { path: PATH });
  const changed = structuredClone(FACTORY);
  changed.args = { literal: "verify" };
  changed.runtime.version = "another-qualified-build";
  const edited = setStepField(parsed.view, "review", "copilotFactory", changed);
  const saved = parseValue(render(edited, parsed.doc, parsed.style)).steps[1];
  const removed = setStepField(parsed.view, "review", "copilotFactory", null);
  assert.deepEqual([
    saved.copilot_factory, saved.inputs_from,
    Object.hasOwn(parseValue(render(removed, parsed.doc, parsed.style)).steps[1], "copilot_factory"),
  ], [changed, ["verify"], false]);
});

test("step rename, insertion, clone and deletion never rewrite static argument strings", () => {
  const parsed = parse(SOURCE, { path: PATH });
  const renamed = renameStep(parsed.view, "verify", "checked");
  const deleted = removeStep(renamed, "checked");
  const clone = scaffoldStep("agent", "copy", { role: "reviewer", copilotFactory: FACTORY });
  deleted.steps.push(clone);
  const saved = parseValue(render(deleted, parsed.doc, parsed.style));
  assert.deepEqual(saved.steps.map((step) => [step.name, step.copilot_factory, step.inputs_from ?? []]),
    [["review", FACTORY, []], ["copy", FACTORY, []]]);
});

test("factory hints reject non-Copilot roles, other step kinds and concurrent members", () => {
  const step = parse(SOURCE, { path: PATH }).view.steps[1];
  const group = { name: "parallel", kind: "concurrent", fields: { members: ["review"] } };
  assert.deepEqual([
    stepFactoryProblems(step, [], ROLES),
    stepFactoryProblems(step, [], [{ name: "reviewer", adapter: "claude" }]).length,
    stepFactoryProblems(step, [], [{ name: "reviewer", adapter: "copilot", permissions: [] }]).length,
    stepFactoryProblems({ ...step, kind: "deterministic" }).length,
    stepFactoryProblems(step, [group], ROLES).length,
  ], [[], 1, 1, 1, 1]);
});

test("save pipeline preserves factory values through the validated draft path", async () => {
  const dir = resolve("factory-memory-fixture");
  const path = join(dir, PATH);
  const files = new Map([[path, SOURCE]]);
  const parsed = parse(SOURCE, { path: PATH });
  const view = editable(parsed.view);
  view.steps[1].fields.copilotFactory = { ...FACTORY, args: null };
  const result = await savePipeline({ dir, pipeline: "factory", view }, {
    readText: async (path) => files.get(path) ?? null,
    writeText: async (path, text) => files.set(path, text),
    validate: async () => ({ state: "validated", ok: true, findings: [] }),
  });
  assert.deepEqual([result.saved, parseValue(files.get(path)).steps[1].copilot_factory],
    [true, { ...FACTORY, args: null }]);
});

test("factory action takes a structured declaration, never a JSON-encoded payload", async () => {
  const action = actions.find((action) => action.name === "set_copilot_factory");
  assert.deepEqual([
    action.inputSchema.additionalProperties,
    action.inputSchema.properties.factory.anyOf[0].properties.runtime.properties.profile.const,
  ], [false, FACTORY_PROFILE]);
  await assert.rejects(() => action.handler({ input: { factory: JSON.stringify(FACTORY) } }),
    /must be an object/u);
});

test("factory action edits a detached draft and can remove misplaced configuration", async () => {
  const drafts = new Map();
  const deps = {
    readText: async () => SOURCE,
    getDraft: (id) => drafts.get(id),
    setDraft: (id, draft) => drafts.set(id, draft),
    validateDraft: async () => ({ ok: true, state: "validated", findings: [] }),
  };
  const action = actions.find((action) => action.name === "set_copilot_factory");
  const input = { dir: resolve("factory-memory-action"), pipeline: "factory", step: "review", factory: structuredClone(FACTORY) };
  const context = { instanceId: "factory-action", input };
  const result = await action.handler(context, deps);
  input.factory.args.nested.count = 999;
  assert.deepEqual([result.dirty, drafts.get(context.instanceId).view.steps[1].fields.copilotFactory], [true, FACTORY]);
  drafts.get(context.instanceId).view.steps[1].kind = "deterministic";
  await action.handler({ ...context, input: { ...input, factory: null } }, deps);
  assert.equal(drafts.get(context.instanceId).view.steps[1].fields.copilotFactory, null);
  await assert.rejects(action.handler({ ...context, input: { ...input, step: "missing" } }, deps), /Unknown step: missing/u);
});
