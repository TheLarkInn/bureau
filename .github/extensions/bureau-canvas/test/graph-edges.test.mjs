// What a React Flow surface says about itself, and what a delete leaves behind.
//
// The first is read by the state matrix's settle rule: a render is finished
// only once every graph on it has drawn the edges it declared. That makes the
// declared count part of a contract rather than a detail — and it has to count
// edges that will actually appear, because React Flow draws nothing for an edge
// whose endpoint is missing and the barrier would then never be reached.
//
// The second is the reason such an edge can exist at all. A step's references
// to another step live in its fields, not only in `view.edges`: `inputs_from`,
// a decision's `on:` map, `over`, and a concurrent step's `members`. Dropping
// the edges alone left those naming a step that had been deleted — and because
// an edge to a missing node draws nothing, the graph looked clean while the
// pipeline carried a dangling reference. `renameStep` has always retargeted all
// four; this is the same obligation on the other operation.
//
// Both the editor's draft delete and the host's saved-view delete read that
// rule from `web/step-refs.mjs`, so it is asserted here once — at the shared
// definition and through `lib/edit.mjs`, the caller that consumes it.
//
// Offline: pure transforms over the committed sample, no browser and no host.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { drawableEdges } from "../web/graph-edges.mjs";
import { withoutReferencesTo } from "../web/step-refs.mjs";
import { editable, removeStep } from "../lib/edit.mjs";
import { pipelineView } from "../lib/view.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);

async function fixtureView() {
  const payload = JSON.parse(await readFile(committedUrl, "utf8"));
  return editable(pipelineView(payload, "agent-eligible-pipeline"));
}

/**
 * The count a graph publishes is what it expects to appear, not what it holds.
 *
 * Table-driven over the shapes an edge list can take against a node set. The
 * two that matter are the dangling ones: counting those would leave the settle
 * barrier waiting for a line React Flow was never going to lay out.
 */
test("a graph declares the edges it can draw, not the ones it was handed", () => {
  const nodes = [{ id: "a" }, { id: "b" }];
  const cases = [
    [[], 0],
    [[{ source: "a", target: "b" }], 1],
    [[{ source: "a", target: "gone" }], 0],
    [[{ source: "gone", target: "b" }], 0],
    [[{ source: "a", target: "b" }, { source: "b", target: "gone" }], 1],
  ];

  assert.deepStrictEqual(
    cases.map(([edges]) => drawableEdges(nodes, edges)),
    cases.map(([, expected]) => expected),
  );
});

/**
 * The sample reaches this in one click: `review` reads `inputs_from:
 * [implement, verify]`, so deleting `verify` used to leave it naming a step
 * that no longer exists — invisibly, because the data edge to it was correctly
 * dropped and nothing else on the screen mentions the field.
 */
test("deleting a step takes the field references that named it", async () => {
  const view = removeStep(await fixtureView(), "verify");

  assert.deepStrictEqual(
    {
      steps: view.steps.map((step) => step.name),
      inputsFrom: view.steps.find((step) => step.name === "review").fields.inputsFrom,
      dangling: view.edges.filter((edge) => edge.source === "verify" || edge.target === "verify").length,
    },
    { steps: ["implement", "review"], inputsFrom: ["implement"], dangling: 0 },
  );
});

/**
 * All four fields a rename retargets, dropped rather than blanked.
 *
 * An outcome with no route and an outcome routed to nothing are different
 * states to this editor — the first is a gap the decision panel offers to fill,
 * the second is a value — and the reader deleted a step, not an outcome.
 */
test("a delete drops every kind of reference a rename would have retargeted", () => {
  const step = {
    name: "gate",
    fields: { over: "verify", on: { success: "verify", failure: "escalate" }, members: ["verify", "review"], inputsFrom: ["verify"] },
  };

  assert.deepStrictEqual(
    withoutReferencesTo(step, "verify").fields,
    { on: { failure: "escalate" }, members: ["review"], inputsFrom: [] },
  );
});

/** A step nothing referenced is handed back with its fields intact. */
test("a delete leaves a step that never named the departed one alone", () => {
  const fields = { over: "apply", on: { success: "done" }, inputsFrom: ["apply"] };

  assert.deepStrictEqual(withoutReferencesTo({ name: "keep", fields }, "verify").fields, fields);
});
