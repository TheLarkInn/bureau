// The pipeline editor's draft transforms, on the side the browser can reach.
//
// `web/editor/editor.mjs` imports React and `@xyflow/react`, so nothing offline
// can import it and ask it anything. That is why the rename guard was held by
// *reading its source*: a test asserted the text `if (to === from ||
// stepNameProblem(view.steps, to, from)) {` appeared in the file. What the
// guard is for — that a refused rename changes nothing — was asserted nowhere,
// and replacing the `return view;` inside it with `void view;` left every
// source predicate at its expected value. A rule tested by its spelling is a
// mark: it fails when the line is reformatted and passes when the refusal is
// deleted, which is both directions the wrong way round.
//
// So the transforms move here, beside `step-refs.mjs` and for its reason: this
// is the one tree both the browser and the offline suite reach, and a rule
// worth writing down is worth being able to run. Pure, and no imports beyond
// the shared field rules.

import { stepNameProblem, withReferencesRetargeted, withoutReferencesTo } from "./step-refs.mjs";

/** A step's outgoing control edges, recomputed from the view's edge list. */
export function syncSteps(view) {
  return {
    ...view,
    steps: view.steps.map((step) => ({ ...step, outgoing: view.edges.filter((edge) => edge.relation === "control" && edge.source === step.name) })),
  };
}

/** The stable identity of an edge, so a retarget does not leave a stale key. */
export function edgeIdentifier(edge) {
  const outcome = edge.outcome ? `:${edge.outcome}` : "";
  return `${edge.relation}:${edge.source}${outcome}->${edge.target}`;
}

/**
 * The step gone, its edges gone, and every reference to it in another step's
 * fields gone with them.
 *
 * The field rule is `step-refs.mjs`, shared with `lib/edit.mjs`, because
 * dangling on delete is one defect however it is reached: a decision routes
 * through its `on:` map and an agent step through `inputs_from`, neither of
 * which is in `view.edges`, so dropping edges alone left the pipeline naming a
 * step that no longer exists. React Flow draws nothing for an edge whose
 * endpoint is missing, so the graph looked clean either way — this is the case
 * the screen cannot show, which is why it has to be true by construction.
 */
export function removeStep(view, stepName) {
  return {
    ...view,
    steps: view.steps.filter((step) => step.name !== stepName).map((step) => withoutReferencesTo(step, stepName)),
    edges: view.edges.filter((edge) => edge.source !== stepName && edge.target !== stepName),
  };
}

/**
 * The draft rename, whose field half is `step-refs.mjs` — the same rule the
 * host's saved-view rename uses, so the two cannot drift the way the delete
 * once did. The name rule is shared from there too: a step may not take a
 * terminal's name, or `on: abort` would stop meaning the terminal it means
 * everywhere it is read, and `StepEditor` reads the same rule so the refusal is
 * explained rather than silent.
 *
 * A refusal returns the view *by identity*, and the caller depends on that:
 * `onRename` compares `next !== view` before it moves the saved position,
 * commits the edit and moves the selection. Returning an equal-but-new object
 * would run all three against a rename that never happened.
 */
export function renameStep(view, from, to) {
  if (to === from || stepNameProblem(view.steps, to, from)) {
    return view;
  }
  const steps = view.steps.map((step) => {
    const retargeted = withReferencesRetargeted(step, from, to);
    return { ...retargeted, id: step.name === from ? to : step.id, name: step.name === from ? to : step.name };
  });
  const edges = view.edges.map((edge) => ({
    ...edge,
    source: edge.source === from ? to : edge.source,
    target: edge.target === from ? to : edge.target,
  })).map((edge) => ({ ...edge, id: edgeIdentifier(edge) }));
  return syncSteps({ ...view, steps, edges });
}
