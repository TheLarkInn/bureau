// Editor transforms on a pipeline view and the hint surface the editor
// shows before a save. `problems` is deliberately advisory — the save path
// owns the verdict.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  createStep,
  decisionGaps,
  edgeTarget,
  editable,
  OUTCOMES,
  problems,
  removeStep,
  renameStep,
  serializable,
  setEdge,
  setStepField,
} from "../lib/edit.mjs";
import { pipelineView } from "../lib/view.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);

async function fixtureView() {
  const payload = JSON.parse(await readFile(committedUrl, "utf8"));
  return editable(pipelineView(payload, "agent-eligible-pipeline"));
}

test("a fresh decision step covers all four outcomes out of the box", async () => {
  const view = createStep(await fixtureView(), "gate", "decision");
  const gate = view.steps.find((step) => step.name === "gate");

  assert.deepEqual(
    { count: view.steps.length, outcomes: Object.keys(gate.fields.on).sort(), over: gate.fields.over, gaps: decisionGaps(gate) },
    { count: 4, outcomes: [...OUTCOMES].sort(), over: "implement", gaps: [] },
  );
});

test("createStep refuses duplicate names and unknown kinds", async () => {
  const view = await fixtureView();

  assert.throws(() => createStep(view, "implement", "agent"), /already has a step/u);
  assert.throws(() => createStep(view, "x", "mystery"), /unknown step kind/u);
});

test("setEdge retargets an outcome and edgeTarget reads it back", async () => {
  const view = setEdge(await fixtureView(), "verify", "failure", "terminal:abort");

  assert.deepEqual(
    { target: edgeTarget(view, "verify", "failure"), kept: edgeTarget(view, "implement", "failure") },
    { target: "abort", kept: "escalate" },
  );
});

test("setEdge with null removes exactly that outcome's edge", async () => {
  const view = setEdge(await fixtureView(), "verify", "failure", null);

  assert.deepEqual(
    {
      removed: edgeTarget(view, "verify", "failure"),
      verifyEdges: view.edges.filter((edge) => edge.source === "verify" && edge.relation === "control").length,
    },
    { removed: null, verifyEdges: 1 },
  );
});

test("setEdge refuses a target that is neither step nor terminal", () => {
  assert.throws(() => setEdge(editableSync(), "a", "success", "ghost"), /no step or terminal/u);
});

function editableSync() {
  return editable({ name: "p", steps: [{ id: "a", name: "a", type: "step", kind: "deterministic", order: 0, fields: {} }], terminals: [], edges: [] });
}

test("removeStep drops the step and every edge that touches it", async () => {
  const view = removeStep(await fixtureView(), "verify");

  assert.deepEqual(
    {
      steps: view.steps.map((step) => step.name),
      dangling: view.edges.filter((edge) => edge.source === "verify" || edge.target === "verify").length,
    },
    { steps: ["implement", "review"], dangling: 0 },
  );
});

test("renameStep retargets edges and field references", async () => {
  const view = renameStep(await fixtureView(), "verify", "check");
  const implementNext = view.edges.find((edge) => edge.source === "implement" && edge.outcome === "success");

  assert.deepEqual(
    { names: view.steps.map((step) => step.name), next: implementNext?.target, inputsFrom: view.steps.find((step) => step.name === "review").fields.inputsFrom },
    { names: ["implement", "check", "review"], next: "check", inputsFrom: ["implement", "check"] },
  );
});

test("decisionGaps reports missing and unknown outcomes from the on map", () => {
  const step = { name: "gate", kind: "decision", fields: { on: { success: "done", weird: "abort" } } };

  assert.deepEqual(decisionGaps(step), ["missing `failure` branch", "missing `blocked` branch", "missing `no-work` branch", "unknown outcome `weird`"]);
});

test("problems flags orphans, dangling edges, and incomplete decisions", async () => {
  let view = await fixtureView();
  view = createStep(view, "floating", "deterministic");
  view = setEdge(view, "floating", "success", "terminal:done");
  view = { ...view, edges: [...view.edges, { id: "control:floating:failure->ghost", source: "floating", target: "ghost", relation: "control", outcome: "failure" }] };
  view = createStep(view, "half", "decision");
  view = setStepField(view, "half", "on", { success: "done" });

  const found = problems(view);

  assert.deepEqual(
    {
      orphan: found.some((item) => item.step === "floating" && /not reachable/u.test(item.message)),
      dangling: found.some((item) => /`ghost`, which does not exist/u.test(item.message)),
      decision: found.filter((item) => item.step === "half").length,
    },
    { orphan: true, dangling: true, decision: 4 },
  );
});

test("serializable strips editor-only decoration", async () => {
  const view = editable(await fixtureView());

  assert.deepEqual(
    serializable(view).steps.map((step) => "outgoing" in step),
    [false, false, false],
  );
});
