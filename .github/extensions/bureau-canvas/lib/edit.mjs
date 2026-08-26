// Edits to a pipeline view before it is saved.
//
// Everything here transforms the `lib/view.mjs` `pipelineView` shape the
// editor works on: steps plus control edges. The codec's `render` already
// diffs such a view against the original document, so an edit that lands
// here round-trips to YAML through the existing machinery rather than a
// parallel serializer.
//
// Nothing here decides whether a pipeline is legal. `problems()` reports
// hints for the editor chrome; the save path owns the final verdict.

import { withoutReferencesTo } from "../web/step-refs.mjs";

export const TERMINALS = ["done", "abort", "escalate"];
export const OUTCOMES = ["success", "failure", "blocked", "no-work"];
export const STEP_KINDS = ["deterministic", "agent", "decision", "concurrent"];

// One control field per non-decision outcome; decision steps route through
// their `on` map instead. Mirrors `CONTROL_FIELDS` in lib/codec.mjs.
export const CONTROL_FIELDS = [
  ["next", "success"],
  ["on_failure", "failure"],
  ["on_blocked", "blocked"],
  ["on_no_work", "no-work"],
];

const TERMINAL_PREFIX = "terminal:";

/** A fresh step, scaffolded with the fields its kind requires. */
export function createStep(view, name, kind) {
  if (!STEP_KINDS.includes(kind)) {
    throw new Error(`unknown step kind \`${kind}\``);
  }
  if (view.steps.some((step) => step.name === name)) {
    throw new Error(`pipeline already has a step named \`${name}\``);
  }
  return {
    ...view,
    steps: [...view.steps, { id: name, name, type: "step", kind, order: view.steps.length, fields: scaffoldFields(view, kind) }],
  };
}

function scaffoldFields(view, kind) {
  const fields = { inputsFrom: [], maxAttempts: 1 };
  if (kind === "deterministic") {
    fields.run = "true";
  }
  if (kind === "decision") {
    fields.over = view.steps[0]?.name ?? "";
    // A decision arrives complete: every outcome covered, each failing
    // closed to `abort` until the editor retargets it.
    fields.on = decisionOn();
  }
  if (kind === "concurrent") {
    fields.members = [];
    fields.completion = "all";
  }
  return fields;
}

/** The `on` map as first-class fields, so the editor edits it directly. */
function decisionOn() {
  return Object.fromEntries(OUTCOMES.map((outcome) => [outcome, "abort"]));
}

/**
 * Removes a step, every edge that touches it, and every reference to it left in
 * another step's fields.
 *
 * The fields are the half this used to miss, and they are the half a rename has
 * always handled: `renamedFields` below retargets `over`, `on`, `members` and
 * `inputsFrom` precisely so a rename never dangles, and a delete has the same
 * obligation. The rule itself lives in `web/step-refs.mjs`, on the side of the
 * served root the browser can reach, so the editor's draft delete and this one
 * are the same rule rather than two copies of it.
 */
export function removeStep(view, name) {
  return {
    ...view,
    steps: view.steps.filter((step) => step.name !== name).map((step) => withoutReferencesTo(step, name)),
    edges: view.edges.filter((edge) => edge.source !== name && edge.target !== name && edge.target !== `${TERMINAL_PREFIX}${name}`),
  };
}

/**
 * Renames a step, retargeting every edge and `over`/members reference so a
 * rename never dangles. Edge ids are derived, so they are rebuilt too.
 */
export function renameStep(view, from, to) {
  if (!to || to === from) {
    return view;
  }
  if (view.steps.some((step) => step.name === to)) {
    throw new Error(`pipeline already has a step named \`${to}\``);
  }
  const steps = view.steps.map((step) => renamedStep(step, from, to));
  return { ...view, steps, edges: view.edges.map((edge) => renamedEdge(edge, from, to)) };
}

function renamedStep(step, from, to) {
  const renamed = step.name === from ? { ...step, id: to, name: to } : { ...step };
  renamed.fields = renamedFields(step, from, to);
  return renamed;
}

function renamedFields(step, from, to) {
  const fields = { ...step.fields };
  if (fields.over === from) {
    fields.over = to;
  }
  if (fields.on && typeof fields.on === "object") {
    fields.on = Object.fromEntries(Object.entries(fields.on).map(([outcome, target]) => [outcome, target === from ? to : target]));
  }
  if (Array.isArray(fields.members)) {
    fields.members = fields.members.map((member) => (member === from ? to : member));
  }
  if (Array.isArray(fields.inputsFrom)) {
    fields.inputsFrom = fields.inputsFrom.map((source) => (source === from ? to : source));
  }
  return fields;
}

function renamedEdge(edge, from, to) {
  const source = edge.source === from ? to : edge.source;
  const target = edge.target === from ? to : edge.target;
  if (source === edge.source && target === edge.target) {
    return edge;
  }
  const next = { ...edge, source, target };
  if (edge.relation === "control") {
    next.id = `control:${source}:${edge.outcome}->${target}`;
  } else if (edge.relation === "data") {
    next.id = `data:${source}->${target}`;
  } else {
    next.id = `observes:${source}->${target}`;
  }
  return next;
}

/** Sets a kind-specific field on one step. */
export function setStepField(view, name, field, value) {
  const steps = view.steps.map((step) =>
    step.name === name ? { ...step, fields: { ...step.fields, [field]: value } } : step,
  );
  return { ...view, steps };
}

/**
 * Points one outcome of one step at a step or terminal; `null` removes the
 * edge. A step emits at most one control edge per outcome, so setting one
 * replaces any existing edge for that outcome.
 */
export function setEdge(view, source, outcome, target) {
  const others = view.edges.filter((edge) => !isControlEdge(edge, source, outcome));
  if (target == null) {
    return { ...view, edges: others };
  }
  const resolved = resolveTarget(view, target);
  return {
    ...view,
    edges: [
      ...others,
      { id: `control:${source}:${outcome}->${resolved}`, source, target: resolved, relation: "control", outcome },
    ],
  };
}

function isControlEdge(edge, source, outcome) {
  return edge.relation === "control" && edge.source === source && edge.outcome === outcome;
}

function resolveTarget(view, target) {
  if (TERMINALS.includes(target)) {
    return `${TERMINAL_PREFIX}${target}`;
  }
  if (target.startsWith(TERMINAL_PREFIX)) {
    return target;
  }
  if (!view.steps.some((step) => step.name === target)) {
    throw new Error(`no step or terminal named \`${target}\``);
  }
  return target;
}

/** The current target of one step outcome, as a step or terminal name. */
export function edgeTarget(view, source, outcome) {
  const edge = view.edges.find((candidate) => isControlEdge(candidate, source, outcome));
  return edge ? plainTarget(edge.target) : null;
}

function plainTarget(target) {
  return target.startsWith(TERMINAL_PREFIX) ? target.slice(TERMINAL_PREFIX.length) : target;
}

/**
 * Decision `on` coverage as a list of gaps: every missing outcome, and every
 * unknown one. A complete decision returns `[]`.
 */
export function decisionGaps(step) {
  if (step.kind !== "decision") {
    return [];
  }
  const covered = coveredOutcomes(step);
  const missing = OUTCOMES.filter((outcome) => !covered.has(outcome)).map((outcome) => `missing \`${outcome}\` branch`);
  const unknown = [...covered].filter((outcome) => !OUTCOMES.includes(outcome)).map((outcome) => `unknown outcome \`${outcome}\``);
  return [...missing, ...unknown];
}

function coveredOutcomes(step) {
  if (step.fields?.on && typeof step.fields.on === "object") {
    return new Set(Object.keys(step.fields.on).filter((outcome) => presentString(step.fields.on[outcome])));
  }
  return new Set((step.outgoing ?? []).map((edge) => edge.outcome));
}

function presentString(value) {
  return typeof value === "string" && value.length > 0;
}

/**
 * Attaches each step's outgoing control edges for editing. The pipeline view
 * keeps edges flat; the editor addresses them per step.
 */
export function editable(view) {
  const steps = view.steps.map((step) => ({
    ...step,
    outgoing: view.edges.filter((edge) => edge.relation === "control" && edge.source === step.name),
  }));
  return { ...view, steps };
}

/** Strips editor-only decoration before the view goes to the codec. */
export function serializable(view) {
  const steps = view.steps.map((step) => {
    const cleaned = { ...step };
    delete cleaned.outgoing;
    return cleaned;
  });
  return { ...view, steps };
}

/**
 * Inline hints, not verdicts: orphan steps, dangling edges, decisions with
 * incomplete `on`. Each problem names the step it belongs to so the editor
 * can mark the node.
 */
export function problems(view) {
  return [
    ...orphanProblems(view),
    ...danglingProblems(view),
    ...view.steps.flatMap((step) => decisionGaps(step).map((message) => ({ step: step.name, message }))),
  ];
}

function orphanProblems(view) {
  const reached = new Set(view.edges.filter((edge) => edge.relation === "control").map((edge) => edge.target));
  return view.steps
    .filter((step, index) => index > 0 && !reached.has(step.name))
    .map((step) => ({ step: step.name, message: "step is not reachable by any control edge" }));
}

function danglingProblems(view) {
  const known = new Set([...view.steps.map((step) => step.name), ...TERMINALS.map((name) => `${TERMINAL_PREFIX}${name}`)]);
  return view.edges
    .filter((edge) => edge.relation === "control" && !known.has(edge.target))
    .map((edge) => ({ step: edge.source, message: `\`${edge.outcome}\` edge points at \`${plainTarget(edge.target)}\`, which does not exist` }));
}
