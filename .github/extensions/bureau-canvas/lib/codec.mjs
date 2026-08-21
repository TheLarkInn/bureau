import * as YAML from "./vendor/yaml.mjs";

const TERMINALS = ["done", "abort", "escalate"];
const OUTCOMES = ["success", "failure", "blocked", "no-work"];
const CONTROL_FIELDS = [
  ["next", "success"],
  ["on_failure", "failure"],
  ["on_blocked", "blocked"],
  ["on_no_work", "no-work"],
];

export function parse(text, options = {}) {
  const style = detectStyle(text);
  const normalized = normalizeLineEndings(text);
  const yamlDoc = YAML.parseDocument(normalized, { keepSourceTokens: true });
  const value = yamlDoc.toJS() ?? {};
  const view = viewFor(value, options.path);
  return { view, doc: { yamlDoc, normalized, view: structuredClone(view) }, style };
}

export function render(view, doc, style) {
  const yamlDoc = YAML.parseDocument(doc.normalized, { keepSourceTokens: true });
  // Field and edge edits address steps by their original index, so they run
  // before any insert or removal shifts those indices.
  applyPipelineChanges(yamlDoc, doc.view, view);
  applyStepStructure(yamlDoc, doc.view, view);
  applyDocumentChanges(yamlDoc, doc.view, view);
  return restoreLineEndings(stringify(yamlDoc, style), style);
}

/**
 * Style for a file this creates. There is no source to detect it from, so it
 * matches what the committed config files use — sequence dashes flush under
 * their key, no flow padding, unwrapped lines — and a created file is then
 * indistinguishable from one `serde_yaml_ng` wrote.
 */
export function createdStyle(lineEnding = "\n") {
  return { lineEnding, finalNewline: true, indentSeq: false, flowCollectionPadding: false, lineWidth: 0 };
}

/** Text for a brand new config file. */
export function createDocument(value, style = createdStyle()) {
  const yamlDoc = new YAML.Document(value);
  return restoreLineEndings(stringify(yamlDoc, style), style);
}

function detectStyle(text) {
  const normalized = normalizeLineEndings(text);
  return {
    lineEnding: text.includes("\r\n") ? "\r\n" : "\n",
    finalNewline: normalized.endsWith("\n"),
    indentSeq: detectIndentedSequences(normalized),
    flowCollectionPadding: detectFlowPadding(normalized),
    lineWidth: 0,
  };
}

function detectIndentedSequences(text) {
  const lines = text.split("\n");
  return lines.some((line, index) => {
    const previous = previousContentLine(lines, index);
    return previous?.trimEnd().endsWith(":") && indentOf(line) > indentOf(previous) && line.trimStart().startsWith("- ");
  });
}

function detectFlowPadding(text) {
  if (/[{[]\s+\S/.test(text) || /\S\s+[}\]]/.test(text)) {
    return true;
  }
  if (/[{[]\S/.test(text) || /\S[}\]]/.test(text)) {
    return false;
  }
  return false;
}

function previousContentLine(lines, index) {
  for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
    if (lines[cursor].trim() && !lines[cursor].trimStart().startsWith("#")) {
      return lines[cursor];
    }
  }
  return null;
}

function indentOf(line) {
  return line.length - line.trimStart().length;
}

function normalizeLineEndings(text) {
  return text.replaceAll("\r\n", "\n");
}

function restoreLineEndings(text, style) {
  const adjusted = style.finalNewline ? text : text.replace(/\n$/, "");
  return style.lineEnding === "\r\n" ? adjusted.replaceAll("\n", "\r\n") : adjusted;
}

function stringify(yamlDoc, style) {
  return yamlDoc.toString({
    flowCollectionPadding: style.flowCollectionPadding,
    indentSeq: style.indentSeq,
    lineWidth: style.lineWidth,
  });
}

function viewFor(value, path) {
  const file = fileItem(value, path);
  if (Array.isArray(value.steps) && typeof value.name === "string") {
    return { ...pipelineItem(value), file };
  }
  return { kind: "document", value, file };
}

function fileItem(value, path) {
  const key = path ? stem(path) : null;
  const declaredName = typeof value?.name === "string" ? value.name : null;
  return {
    path: path ?? null,
    key,
    declaredName,
    nameMismatch: Boolean(key && declaredName && key !== declaredName),
  };
}

function stem(path) {
  return path.replaceAll("\\", "/").split("/").at(-1).replace(/\.(ya?ml)$/u, "");
}

function pipelineItem(pipeline) {
  return {
    kind: "pipeline",
    name: pipeline.name,
    steps: (pipeline.steps ?? []).map(stepItem),
    terminals: TERMINALS.map((name) => ({ id: `terminal:${name}`, name, type: "terminal" })),
    edges: (pipeline.steps ?? []).flatMap(stepEdges),
  };
}

function stepItem(step, order) {
  return {
    id: step.name,
    name: step.name,
    type: "step",
    kind: step.type,
    order,
    fields: {
      inputsFrom: step.inputs_from ?? [],
      maxAttempts: step.max_attempts ?? 1,
      ...presentFields(step),
    },
  };
}

function presentFields(step) {
  const fields = {};
  for (const [target, source] of [
    ["run", "run"],
    ["role", "role"],
    ["fixture", "fixture"],
    ["trust", "trust"],
    ["over", "over"],
    ["members", "steps"],
    ["completion", "completion"],
    ["maxConcurrent", "max_concurrent"],
    ["timeoutSecs", "timeout_secs"],
  ]) {
    if (source in step) {
      fields[target] = step[source];
    }
  }
  return fields;
}

function stepEdges(step) {
  return [...controlEdges(step), ...dataEdges(step), ...observesEdges(step)];
}

function controlEdges(step) {
  return [...namedControlEdges(step), ...decisionControlEdges(step)];
}

function namedControlEdges(step) {
  return CONTROL_FIELDS.flatMap(([field, outcome]) => (presentTarget(step[field]) ? [controlEdge(step.name, step[field], outcome)] : []));
}

function decisionControlEdges(step) {
  if (step.type !== "decision") {
    return [];
  }
  return OUTCOMES.flatMap((outcome) => (presentTarget(step.on?.[outcome]) ? [controlEdge(step.name, step.on[outcome], outcome)] : []));
}

function dataEdges(step) {
  return (step.inputs_from ?? []).filter(presentTarget).map((source) => ({
    id: `data:${source}->${step.name}`,
    source,
    target: step.name,
    relation: "data",
  }));
}

function observesEdges(step) {
  return step.type === "decision" && presentTarget(step.over)
    ? [{ id: `observes:${step.over}->${step.name}`, source: step.over, target: step.name, relation: "observes" }]
    : [];
}

function controlEdge(source, target, outcome) {
  const resolvedTarget = TERMINALS.includes(target) ? `terminal:${target}` : target;
  return { id: `control:${source}:${outcome}->${resolvedTarget}`, source, target: resolvedTarget, relation: "control", outcome };
}

function presentTarget(target) {
  return typeof target === "string" && target.length > 0;
}

function applyPipelineChanges(yamlDoc, original, next) {
  if (original.kind !== "pipeline" || next.kind !== "pipeline") {
    return;
  }
  const stepIndex = new Map(original.steps.map((step, index) => [step.name, index]));
  const originalEdges = controlEdgeMap(original.edges);
  const nextEdges = controlEdgeMap(next.edges);
  applyStepFieldChanges(yamlDoc, stepIndex, original.steps, next.steps);
  removeDeletedEdges(yamlDoc, stepIndex, originalEdges, nextEdges, original.steps);
  upsertChangedEdges(yamlDoc, stepIndex, originalEdges, nextEdges, original.steps);
}

function applyDocumentChanges(yamlDoc, original, next) {
  if (original.kind !== "document" || next.kind !== "document") {
    return;
  }
  const before = original.value ?? {};
  const after = next.value ?? {};
  for (const key of Object.keys(before)) {
    if (!(key in after)) {
      yamlDoc.delete(key);
    }
  }
  for (const [key, value] of Object.entries(after)) {
    applyValueChange(yamlDoc, [key], before[key], value);
  }
}

/**
 * Diffs collections element by element rather than replacing the whole node.
 * Replacing would rewrite every sibling entry, and a save that rewrites the
 * file makes the review PR useless (DESIGN.md section 5).
 */
function applyValueChange(yamlDoc, path, before, after) {
  if (!changedValue(before, after)) {
    return;
  }
  if (isPlainMap(before) && isPlainMap(after)) {
    applyMapChange(yamlDoc, path, before, after);
  } else if (Array.isArray(before) && Array.isArray(after)) {
    applySequenceChange(yamlDoc, path, before, after);
  } else {
    yamlDoc.setIn(path, after);
  }
}

function applyMapChange(yamlDoc, path, before, after) {
  for (const key of Object.keys(before)) {
    if (!(key in after)) {
      yamlDoc.deleteIn([...path, key]);
    }
  }
  for (const [key, value] of Object.entries(after)) {
    applyValueChange(yamlDoc, [...path, key], before[key], value);
  }
}

/** Removes dropped items from the back, then inserts added ones in place. */
function applySequenceChange(yamlDoc, path, before, after) {
  // The passes below are set-based, so a pure reorder would be a no-op. Where
  // order carries meaning — an assignment's `repos`, whose first entry is the
  // repo the branch lands in — the sequence is rewritten instead.
  if (reordered(before, after)) {
    yamlDoc.setIn(path, after);
    return;
  }
  const kept = new Set(after.map(serialize));
  for (let index = before.length - 1; index >= 0; index -= 1) {
    if (!kept.has(serialize(before[index]))) {
      yamlDoc.deleteIn([...path, index]);
    }
  }
  const remaining = before.filter((item) => kept.has(serialize(item)));
  for (const [index, item] of after.entries()) {
    if (!remaining.some((existing) => serialize(existing) === serialize(item))) {
      insertAt(yamlDoc, path, index, item);
    }
  }
  if (after.length === 0) {
    yamlDoc.setIn(path, []);
  }
}

/** Whether the items common to both lists appear in a different order. */
function reordered(before, after) {
  const wanted = after.map(serialize);
  const kept = before.map(serialize).filter((item) => wanted.includes(item));
  const target = wanted.filter((item) => kept.includes(item));
  return kept.some((item, index) => item !== target[index]);
}

function insertAt(yamlDoc, path, index, item) {  const sequence = yamlDoc.getIn(path);
  const node = yamlDoc.createNode(item);
  if (sequence?.items && index <= sequence.items.length) {
    sequence.items.splice(index, 0, node);
  } else {
    yamlDoc.addIn(path, node);
  }
}

function isPlainMap(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function serialize(value) {
  return JSON.stringify(value);
}

function changedValue(left, right) {
  return JSON.stringify(left) !== JSON.stringify(right);
}

/** Adds and removes whole steps, leaving every untouched step's lines alone. */
function applyStepStructure(yamlDoc, original, next) {
  if (original.kind !== "pipeline" || next.kind !== "pipeline") {
    return;
  }
  const keep = new Set(next.steps.map((step) => step.name));
  for (let index = original.steps.length - 1; index >= 0; index -= 1) {
    if (!keep.has(original.steps[index].name)) {
      yamlDoc.deleteIn(["steps", index]);
    }
  }
  const present = new Set(original.steps.filter((step) => keep.has(step.name)).map((step) => step.name));
  for (const [index, step] of next.steps.entries()) {
    if (!present.has(step.name)) {
      insertAt(yamlDoc, ["steps"], index, rawStep(step, next.edges ?? []));
    }
  }
  if (next.steps.length === 0) {
    yamlDoc.set("steps", []);
  }
}

/**
 * A step view converted back to `StepDef` field order. Only fields that are
 * present are written: an absent outcome fails closed to `abort`, so writing
 * one would change what the file says to a reviewer.
 */
function rawStep(step, edges) {
  const raw = { name: step.name, type: step.kind };
  for (const [target, source] of RAW_STEP_FIELDS) {
    if (step.fields?.[target] != null) {
      raw[source] = step.fields[target];
    }
  }
  Object.assign(raw, rawStepEdges(step, edges));
  if ((step.fields?.inputsFrom ?? []).length > 0) {
    raw.inputs_from = step.fields.inputsFrom;
  }
  if ((step.fields?.maxAttempts ?? 1) !== 1) {
    raw.max_attempts = step.fields.maxAttempts;
  }
  return raw;
}

function rawStepEdges(step, edges) {
  const outgoing = edges.filter((edge) => edge.source === step.name && edge.relation === "control");
  const raw = {};
  for (const [field, outcome] of CONTROL_FIELDS) {
    const edge = outgoing.find((candidate) => candidate.outcome === outcome);
    if (edge) {
      raw[field] = targetName(edge.target);
    }
  }
  if (step.kind === "decision") {
    const on = Object.fromEntries(outgoing.map((edge) => [edge.outcome, targetName(edge.target)]));
    return Object.keys(on).length > 0 ? { on } : {};
  }
  return raw;
}

const RAW_STEP_FIELDS = [
  ["run", "run"],
  ["role", "role"],
  ["fixture", "fixture"],
  ["trust", "trust"],
  ["over", "over"],
  ["members", "steps"],
  ["completion", "completion"],
  ["maxConcurrent", "max_concurrent"],
  ["timeoutSecs", "timeout_secs"],
];

function applyStepFieldChanges(yamlDoc, stepIndex, originalSteps, nextSteps) {
  const originalByName = new Map(originalSteps.map((step) => [step.name, step]));
  for (const step of nextSteps) {
    const original = originalByName.get(step.name);
    const index = stepIndex.get(step.name);
    if (!original || index == null) {
      continue;
    }
    for (const [viewField, yamlField] of mutableFields()) {
      if (step.fields?.[viewField] !== original.fields?.[viewField]) {
        yamlDoc.setIn(["steps", index, yamlField], step.fields?.[viewField] ?? null);
      }
    }
  }
}

function mutableFields() {
  return [
    ["run", "run"],
    ["role", "role"],
    ["trust", "trust"],
    ["over", "over"],
    ["maxAttempts", "max_attempts"],
    ["timeoutSecs", "timeout_secs"],
  ];
}

function controlEdgeMap(edges) {
  return new Map(edges.filter((edge) => edge.relation === "control").map((edge) => [`${edge.source}:${edge.outcome}`, edge]));
}

function removeDeletedEdges(yamlDoc, stepIndex, originalEdges, nextEdges, steps) {
  for (const [key, edge] of originalEdges) {
    if (!nextEdges.has(key)) {
      deleteControl(yamlDoc, stepIndex.get(edge.source), steps, edge.outcome);
    }
  }
}

function upsertChangedEdges(yamlDoc, stepIndex, originalEdges, nextEdges, steps) {
  for (const [key, edge] of nextEdges) {
    const original = originalEdges.get(key);
    if (!original || original.target !== edge.target) {
      setControl(yamlDoc, stepIndex.get(edge.source), steps, edge.outcome, targetName(edge.target));
    }
  }
}

function deleteControl(yamlDoc, index, steps, outcome) {
  if (index == null) {
    return;
  }
  const field = controlField(steps[index], outcome);
  yamlDoc.deleteIn(field.path(index));
}

function setControl(yamlDoc, index, steps, outcome, target) {
  if (index == null) {
    return;
  }
  const field = controlField(steps[index], outcome);
  yamlDoc.setIn(field.path(index), target);
}

function controlField(step, outcome) {
  if (step.kind === "decision") {
    return { path: (index) => ["steps", index, "on", outcome] };
  }
  const field = CONTROL_FIELDS.find(([, candidate]) => candidate === outcome)?.[0];
  return { path: (index) => ["steps", index, field] };
}

function targetName(target) {
  return target.startsWith("terminal:") ? target.slice("terminal:".length) : target;
}
