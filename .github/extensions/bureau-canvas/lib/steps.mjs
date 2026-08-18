// Adding, removing and reordering steps inside a pipeline.
//
// A pipeline's step list is not an ordinary list. Order is semantic — `over`
// and `inputs_from` must name earlier steps — and the FIRST step is the entry
// step, so removing or displacing it silently re-points the whole pipeline.
// That case is reported distinctly rather than treated as an ordinary delete.
//
// Legality is still `bureau validate`'s call; this module reports consequences
// and produces text.

import { CHOICES } from "./entities.mjs";

const CONTROL_FIELDS = [
  ["next", "success"],
  ["on_failure", "failure"],
  ["on_blocked", "blocked"],
  ["on_no_work", "no-work"],
];

/**
 * A step that is valid the moment it exists. Only required fields are set:
 * `max_attempts` defaults to 1 and an absent outcome already fails closed to
 * `abort`, so writing either would change what the file says to a reviewer.
 */
export function scaffoldStep(kind, name, fields = {}) {
  if (!CHOICES.stepKind.includes(kind)) {
    throw new Error(`unknown step kind \`${kind}\``);
  }
  const step = { id: name, name, type: "step", kind, order: 0, fields: { inputsFrom: [], maxAttempts: 1 } };
  Object.assign(step.fields, requiredStepFields(kind, fields));
  return step;
}

function requiredStepFields(kind, fields) {
  if (kind === "deterministic") {
    return { run: fields.run ?? "true" };
  }
  if (kind === "agent") {
    return { role: fields.role ?? "" };
  }
  if (kind === "decision") {
    return { over: fields.over ?? "" };
  }
  return { members: fields.members ?? [], maxConcurrent: fields.maxConcurrent ?? undefined };
}

/** A decision's `on` must cover all four outcomes, so scaffold all four. */
export function scaffoldStepEdges(step, target = "abort") {
  if (step.kind !== "decision") {
    return [edge(step.name, "success", target)];
  }
  return CHOICES.outcome.map((outcome) => edge(step.name, outcome, target));
}

function edge(source, outcome, target) {
  const id = target.startsWith("terminal:") ? target : `terminal:${target}`;
  return { id: `control:${source}:${outcome}->${id}`, source, target: id, relation: "control", outcome };
}

/** Inserts a step at `index`, keeping every `order` consistent afterwards. */
export function insertStep(view, step, index = view.steps.length, edges = []) {
  const next = structuredClone(view);
  const at = Math.max(0, Math.min(index, next.steps.length));
  next.steps.splice(at, 0, structuredClone(step));
  next.steps.forEach((item, order) => {
    item.order = order;
  });
  next.edges = [...(next.edges ?? []), ...edges];
  return next;
}

export function removeStep(view, name) {
  const next = structuredClone(view);
  next.steps = next.steps.filter((step) => step.name !== name);
  next.steps.forEach((step, order) => {
    step.order = order;
  });
  next.edges = (next.edges ?? []).filter((candidate) => candidate.source !== name && candidate.target !== name);
  for (const step of next.steps) {
    step.fields.inputsFrom = (step.fields.inputsFrom ?? []).filter((input) => input !== name);
    if (step.fields.members) {
      step.fields.members = step.fields.members.filter((member) => member !== name);
    }
  }
  return next;
}

export function moveStep(view, name, index) {
  const from = view.steps.findIndex((step) => step.name === name);
  if (from < 0) {
    return view;
  }
  const next = structuredClone(view);
  const [step] = next.steps.splice(from, 1);
  next.steps.splice(Math.max(0, Math.min(index, next.steps.length)), 0, step);
  next.steps.forEach((item, order) => {
    item.order = order;
  });
  return next;
}

/**
 * What a step edit costs, before it is applied. The entry-step case is its own
 * severity because it changes where every run starts, while looking exactly
 * like an ordinary delete.
 */
export function stepConsequences(view, name, action = "delete") {
  const index = view.steps.findIndex((step) => step.name === name);
  if (index < 0) {
    return [];
  }
  const found = [...entryConsequence(view, index, action), ...referrerConsequences(view, name)];
  return found;
}

function entryConsequence(view, index, action) {
  if (index !== 0) {
    return [];
  }
  const successor = view.steps[1]?.name ?? null;
  const detail = successor
    ? `\`${view.steps[0].name}\` is the entry step; \`${successor}\` would become the entry`
    : `\`${view.steps[0].name}\` is the only step; the pipeline would have none`;
  return [{ severity: "entry-step", action, step: view.steps[0].name, successor, message: detail }];
}

function referrerConsequences(view, name) {
  const found = [];
  for (const edge of view.edges ?? []) {
    if (edge.target === name) {
      found.push({ severity: "referrer", step: edge.source, relation: edge.relation, message: referrerMessage(edge, name) });
    }
  }
  for (const step of view.steps) {
    if ((step.fields?.members ?? []).includes(name)) {
      found.push({ severity: "referrer", step: step.name, relation: "member", message: `concurrent group \`${step.name}\` lists \`${name}\`` });
    }
  }
  return found;
}

function referrerMessage(edge, name) {
  if (edge.relation === "control") {
    return `step \`${edge.source}\` routes \`${edge.outcome}\` to \`${name}\``;
  }
  return `step \`${edge.source}\` takes \`${name}\` as ${edge.relation === "data" ? "input" : "the observed step"}`;
}

/** Steps that become unreachable from the entry once `name` is gone. */
export function orphanedBy(view, name) {
  const after = removeStep(view, name);
  const reachable = reachableFrom(after);
  return after.steps.filter((step) => !reachable.has(step.name)).map((step) => step.name);
}

function reachableFrom(view) {
  const entry = view.steps[0];
  const seen = new Set();
  if (!entry) {
    return seen;
  }
  const queue = [entry.name];
  const byName = new Map(view.steps.map((step) => [step.name, step]));
  while (queue.length > 0) {
    const current = queue.shift();
    if (seen.has(current) || !byName.has(current)) {
      continue;
    }
    seen.add(current);
    queue.push(...successors(view, byName.get(current)));
  }
  return seen;
}

function successors(view, step) {
  const targets = (view.edges ?? [])
    .filter((edge) => edge.source === step.name && edge.relation === "control")
    .map((edge) => edge.target.replace(/^terminal:/u, ""));
  return [...targets, ...(step.fields?.members ?? [])];
}

export function controlFields() {
  return CONTROL_FIELDS.map(([field, outcome]) => ({ field, outcome }));
}
