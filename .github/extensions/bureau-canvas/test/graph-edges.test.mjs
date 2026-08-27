// What a React Flow surface says about itself, and what a delete leaves behind.
//
// The first is read by the state matrix's settle rule: a render is finished
// only once every graph on it has drawn the edges it declared. That makes the
// declared count part of a contract rather than a detail — and it has to count
// edges that will actually appear, because React Flow draws nothing for an edge
// whose endpoint is missing and the barrier would then never be reached.
//
// It also has to be counted from the surface's own model rather than from the
// arrays handed to the renderer. As a pure timing barrier either source would
// do, since both move together by construction; but `undrawn-graph` turned the
// number into a claim about the screen, and a projection that dropped every
// edge answered that claim by lowering the bar — declaring 0, drawing 0, and
// passing. Independence is a property of the call site, so it is asserted here
// against all three surfaces that publish the attribute.
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
import { parse, render } from "../lib/codec.mjs";
import { editable, removeStep } from "../lib/edit.mjs";
import { pipelineView } from "../lib/view.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);
const referenceUrl = new URL("./fixtures/codec-reference-pipeline.yaml", import.meta.url);

/** The three surfaces that publish `data-graph-edges` about themselves. */
const SURFACES = ["app.mjs", "editor/editor.mjs", "editor/relation.mjs"];

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
 * The number a graph publishes about itself is not read off the array it handed
 * the renderer.
 *
 * `undrawn-graph` fails a render whose graph never drew the edges it declared.
 * While both numbers came from the same projection that failure could not fire
 * for the defect it names: a `toFlow` that dropped every edge declared 0, drew
 * 0, and satisfied `graphsDrawn` and `undrawnGraphs` alike, so the gallery would
 * publish graphs of disconnected boxes and the matrix would stay green. The
 * count is only evidence if it is derived independently, which is a property of
 * the *call site* — nothing inside `drawableEdges` can enforce it, so it is
 * asserted here, offline, by reading how each surface computes the attribute.
 *
 * Checked for all three at once rather than only the relation graph, because
 * the same shape was on the pipeline and the editor, and a rule held at one of
 * three call sites is the kind of half-measure this whole registry exists to
 * remove. `publishes` and `counts` guard the rule against going vacuous if a
 * surface stops publishing or stops counting altogether.
 */
test("no surface counts its edges from the arrays it handed React Flow", async () => {
  const sources = await Promise.all(SURFACES.map((path) => readFile(new URL(`../web/${path}`, import.meta.url), "utf8")));
  const calls = sources.map((source) =>
    source.split("\n").filter((line) => line.includes("drawableEdges(") && !line.trimStart().startsWith("*")));

  assert.deepStrictEqual(
    sources.map((source, index) => ({
      publishes: source.includes("data-graph-edges"),
      counts: calls[index].length > 0,
      fromProjection: calls[index].some((line) => /\bflow\.(nodes|edges)\b/u.test(line)),
    })),
    SURFACES.map(() => ({ publishes: true, counts: true, fromProjection: false })),
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
 * All four fields a rename retargets, dropped rather than blanked — except
 * `over`, which is blanked on purpose.
 *
 * The codec reads `undefined` as "this edit says nothing about that field", so
 * deleting `over` outright wrote nothing and left the file naming the departed
 * step. `null` is the codec's word for "remove the key", and every reader takes
 * it exactly as it takes an absent one.
 *
 * For `on`, dropping is the point: an outcome with no route and an outcome
 * routed to nothing are different states to this editor — the first is a gap
 * the decision panel offers to fill, the second is a value — and the reader
 * deleted a step, not an outcome.
 */
test("a delete drops every kind of reference a rename would have retargeted", () => {
  const step = {
    name: "gate",
    fields: { over: "verify", on: { success: "verify", failure: "escalate" }, members: ["verify", "review"], inputsFrom: ["verify"] },
  };

  assert.deepStrictEqual(
    withoutReferencesTo(step, "verify").fields,
    { over: null, on: { failure: "escalate" }, members: ["review"], inputsFrom: [] },
  );
});

/** A step nothing referenced is handed back with its fields intact. */
test("a delete leaves a step that never named the departed one alone", () => {
  const fields = { over: "apply", on: { success: "done" }, inputsFrom: ["apply"] };

  assert.deepStrictEqual(withoutReferencesTo({ name: "keep", fields }, "verify").fields, fields);
});

/** The reference fixture, re-rendered with one change applied to its steps. */
async function renderWith(change) {
  const parsed = parse(await readFile(referenceUrl, "utf8"), { path: "codec-reference-pipeline.yaml" });
  const next = structuredClone(parsed.view);
  next.steps = next.steps.map(change);
  return render(next, parsed.doc, parsed.style);
}

/**
 * The write, not only the view — which is the half `over` was missing.
 *
 * The codec reads `undefined` as "this edit says nothing about that field" and
 * `null` as "remove the key". So dropping `over` produced a view with no
 * observation and a *file* that still said `over: propose`, naming the step
 * that had just been deleted. Nothing on screen shows it, and the report comes
 * back later out of a validation with nothing to connect it to the click.
 *
 * Both directions in one assertion, because the defect is exactly the
 * difference between them: the fixture's `review` observes `propose`, and the
 * two renders differ only in how its removal was expressed.
 */
test("deleting an observed step takes `over` out of the file, not just the view", async () => {
  const dropped = await renderWith((step) =>
    (step.name === "review" ? { ...step, fields: { ...step.fields, over: undefined } } : step));
  const nulled = await renderWith((step) => withoutReferencesTo(step, "propose"));

  assert.deepStrictEqual(
    [dropped.includes("over: propose"), nulled.includes("over: propose")],
    [true, false],
  );
});
