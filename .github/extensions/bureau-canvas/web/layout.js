// Step-graph layout for the pipeline editor.
//
// Pure and deterministic: same view in, same coordinates out, so a re-render
// never moves a node the user did not move. Layers follow control edges in
// declaration order — a forward edge pushes its target at least one layer
// past its source; back edges add nothing. Terminals sit on their own rail
// to the right.

const X_GAP = 320;
const Y_GAP = 190;
const TERMINAL_GAP = 140;
const OUTCOME_ORDER = ["success", "failure", "blocked", "no-work"];

/** Layout for one pipeline view (`lib/view.mjs` `pipelineView` shape). */
export function layoutPipeline(view) {
  const steps = orderedSteps(view);
  const layers = stepLayers(view, steps);
  const nodes = placeSteps(steps, layers);
  const terminals = placeTerminals(view, nodes);
  return {
    nodes: [...nodes, ...terminals],
    edges: (view.edges ?? []).map((edge) => ({ ...edge })),
  };
}

function orderedSteps(view) {
  return [...(view.steps ?? [])].sort(
    (left, right) => (left.order ?? 0) - (right.order ?? 0) || left.id.localeCompare(right.id),
  );
}

function stepLayers(view, steps) {
  const order = new Map(steps.map((step) => [step.id, step.order ?? 0]));
  const layers = new Map(steps.map((step) => [step.id, 0]));
  const outgoing = controlBySource(view.edges ?? []);
  for (const step of steps) {
    raiseMembers(step, layers, order);
    raiseTargets(step, layers, order, outgoing.get(step.id) ?? []);
  }
  return layers;
}

function controlBySource(edges) {
  const bySource = new Map();
  for (const edge of edges.filter((item) => item.relation === "control")) {
    bySource.set(edge.source, [...(bySource.get(edge.source) ?? []), edge]);
  }
  return bySource;
}

function raiseMembers(step, layers, order) {
  for (const member of step.fields?.members ?? []) {
    if (order.has(member)) {
      layers.set(member, Math.max(layers.get(member) ?? 0, (layers.get(step.id) ?? 0) + 1));
    }
  }
}

function raiseTargets(step, layers, order, edges) {
  const members = step.fields?.members ?? [];
  const from = Math.max(layers.get(step.id) ?? 0, ...members.map((member) => layers.get(member) ?? 0));
  for (const edge of [...edges].sort(edgeOrder)) {
    if (order.has(edge.target) && order.get(edge.target) > order.get(step.id)) {
      layers.set(edge.target, Math.max(layers.get(edge.target) ?? 0, from + 1));
    }
  }
}

function edgeOrder(left, right) {
  return outcomeRank(left.outcome) - outcomeRank(right.outcome) || left.id.localeCompare(right.id);
}

function outcomeRank(outcome) {
  const rank = OUTCOME_ORDER.indexOf(outcome ?? "");
  return rank === -1 ? OUTCOME_ORDER.length : rank;
}

function placeSteps(steps, layers) {
  const slots = new Map();
  return steps.map((step) => {
    const layer = layers.get(step.id) ?? 0;
    const column = slots.get(layer) ?? 0;
    slots.set(layer, column + 1);
    return { id: step.id, kind: step.kind, step, x: column * X_GAP, y: layer * Y_GAP, layer, column };
  });
}

function placeTerminals(view, steps) {
  const rail = Math.max(0, ...steps.map((step) => step.column)) + 1;
  const rows = new Map(steps.map((step) => [step.id, step.layer]));
  return (view.terminals ?? []).map((terminal, index) => ({
    id: terminal.id,
    name: terminal.name,
    x: rail * X_GAP + TERMINAL_GAP,
    y: terminalLayer(terminal.id, view.edges ?? [], rows, index) * Y_GAP,
  }));
}

function terminalLayer(id, edges, rows, fallback) {
  const feeding = edges
    .filter((edge) => edge.relation === "control" && edge.target === id)
    .map((edge) => rows.get(edge.source))
    .filter((row) => row != null);
  return feeding.length === 0 ? fallback : Math.max(...feeding) + 1;
}
