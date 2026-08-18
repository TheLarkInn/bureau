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
  applyPipelineChanges(yamlDoc, doc.view, view);
  applyDocumentChanges(yamlDoc, doc.view, view);
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
  for (const [key, value] of Object.entries(next.value ?? {})) {
    if (changedValue(value, original.value?.[key])) {
      yamlDoc.set(key, value);
    }
  }
}

function changedValue(left, right) {
  return JSON.stringify(left) !== JSON.stringify(right);
}

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
