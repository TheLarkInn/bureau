import assert from "node:assert/strict";
import test from "node:test";
import { operationsOverview } from "../web/operations.mjs";
import { OPERATIONS_VISUAL_NOW, operationsVisualFixture } from "./support/operations-visual.mjs";

const BASE = {
  dir: "(bundled sample)", status: "Sample", validation: { state: "fixture", checked_at_ms: null },
  authoring: { state: "sample", commit: null, changes: null },
  pipeline: "repair", selectedPipeline: { name: "repair" }, navigation: { view: "pipeline" },
  pipelines: { repair: {} },
  config: { view: { assignments: [{ name: "fix-tests", pipeline: "repair", limits: {}, work: {} }] } },
};

test("Operations screenshots select their own surface without changing the pinned host sample", () => {
  const before = structuredClone(BASE);
  const { state } = operationsVisualFixture(BASE);
  assert.deepEqual([state.pipeline, state.selectedPipeline, state.navigation, state.validation.checked_at_ms],
    [null, null, { view: "operations" }, null]);
  assert.deepEqual(BASE, before);
});

test("the visual clock pins a recorded validation read but never invents one for a sample", () => {
  const checked = { ...BASE, validation: { state: "validated", ok: true, checked_at_ms: 1 } };
  assert.deepEqual([operationsVisualFixture(checked).state.validation.checked_at_ms,
    operationsVisualFixture(BASE).state.validation.checked_at_ms], [OPERATIONS_VISUAL_NOW, null]);
});

test("Operations screenshot counts come from readable, paused, failed, stale, and corrupt logs", () => {
  const { state, listing } = operationsVisualFixture(BASE);
  const model = operationsOverview(state, listing, OPERATIONS_VISUAL_NOW);
  assert.deepEqual(model.counts, { attention: 4, active: 1, paused: 1, failed: 1 });
  assert.deepEqual(model.runs.map((run) => [run.run_id, run.label, run.cost]), [
    ["failed-run", "Failed", "Unknown"],
    ["paused-run", "Paused", "Unknown"],
    ["stale-run", "Unfinished, stale evidence", "Unknown"],
    ["corrupt-log", "Evidence unavailable", "Unknown"],
    ["active-run", "Active (observed)", "Unknown"],
  ]);
});

test("corrupt screenshot evidence cannot retain a successful prefix or a safe run link", () => {
  const { state, listing } = operationsVisualFixture(BASE);
  const model = operationsOverview(state, listing, OPERATIONS_VISUAL_NOW);
  const corrupt = model.runs.find((run) => run.run_id === "corrupt-log");
  assert.match(corrupt.evidence.message, /Invalid JSON/u);
  assert.deepEqual([corrupt.evidence.state, corrupt.evidence.outcome, corrupt.evidence.config_source, corrupt.target],
    ["invalid", null, null, null]);
  assert.deepEqual([model.configuration.verdict, model.configuration.git.commit, model.recorded_sources.length],
    ["Sample configuration", null, 1]);
});

test("unavailable screenshot evidence stays unknown beside invalid read-only configuration", () => {
  const { state, listing } = operationsVisualFixture(BASE, "unavailable");
  const model = operationsOverview(state, listing, OPERATIONS_VISUAL_NOW);
  assert.deepEqual([model.access.mode, model.configuration.verdict, model.configuration.errors],
    ["read-only", "Invalid configuration", ["assignments/work.yaml: invalid YAML"]]);
  assert.deepEqual([model.counts, model.observation.state, model.runs, model.assignments], [null, "missing", [], []]);
});

test("visual fixtures reject an unknown variant or missing pipeline rather than inventing evidence", () => {
  assert.throws(() => operationsVisualFixture(BASE, "unknown"), /Unknown Operations visual fixture/u);
  assert.throws(() => operationsVisualFixture({ ...BASE, pipelines: {} }), /configured assignment and pipeline/u);
});
