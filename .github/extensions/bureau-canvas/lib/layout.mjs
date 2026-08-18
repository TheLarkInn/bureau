const X_GAP = 240;
const Y_GAP = 120;
const TERMINAL_GAP = 180;
const CONFIG_COLUMNS = {
  "work-source": 0,
  assignment: 1,
  role: 2,
  repo: 2,
  pipeline: 3,
};
const TERMINAL_PREFIX = "terminal:";
const RELATION_ORDER = { control: 0, data: 1, observes: 2 };
const ROUTES = { data: "data", observes: "observes" };

export function pipelineLayout(view, arrangement = {}) {
  const steps = stepsByOrder(view.steps ?? []);
  const rows = stepRows(view, steps);
  const placedSteps = placeSteps(steps, rows);
  const placedTerminals = placeTerminals(view, placedSteps);
  const moved = mergeArrangement({ name: view.name, steps: placedSteps, terminals: placedTerminals }, arrangement);
  const edges = placePipelineEdges(view, moved.steps, moved.terminals);
  return { ...moved, edges };
}

export function configLayout(view, arrangement = {}) {
  const main = configMainItems(view);
  const orphans = configOrphanItems(view, main.length);
  const items = [...main, ...orphans].map(placeConfigItem);
  const edges = configEdges(view, new Set(items.map((item) => item.id)));
  return mergeArrangement({ dir: view.dir, items, edges }, arrangement);
}

export function arrangementItemKey(configPath, itemName) {
  return `${configPath}\u001f${itemName}`;
}

export function arrangementBucket(configPath) {
  return hashText(configPath ?? "");
}

export function mergeArrangement(layout, arrangement = {}) {
  const positions = arrangement.positions ?? {};
  return mapPlacedItems(layout, (item) => moveItem(item, positions[item.arrangementKey ?? item.id]));
}

function stepsByOrder(steps) {
  assertUnique(steps.map((step) => step.id), "step");
  return [...steps].sort((left, right) => left.order - right.order || left.id.localeCompare(right.id));
}

function assertUnique(values, kind) {
  const seen = new Set();
  for (const value of values) {
    if (seen.has(value)) {
      throw new Error(`duplicate ${kind} id: ${value}`);
    }
    seen.add(value);
  }
}

function stepRows(view, steps) {
  const order = new Map(steps.map((step) => [step.id, step.order]));
  const rows = new Map(steps.map((step) => [step.id, 0]));
  const outgoing = controlBySource(view.edges ?? []);
  for (const step of steps) {
    placeMembers(step, rows, order);
    placeForwardTargets(step, rows, order, outgoing.get(step.id) ?? []);
  }
  return rows;
}

function controlBySource(edges) {
  const bySource = new Map();
  for (const edge of edges.filter((edge) => edge.relation === "control")) {
    const list = bySource.get(edge.source) ?? [];
    list.push(edge);
    bySource.set(edge.source, list);
  }
  return bySource;
}

function placeMembers(step, rows, order) {
  const sourceRow = rows.get(step.id) ?? 0;
  for (const member of step.fields?.members ?? []) {
    if (isForwardStep(step.id, member, order)) {
      raiseRow(rows, member, sourceRow + 1);
    }
  }
}

function placeForwardTargets(step, rows, order, edges) {
  const sourceRow = routingRow(step, rows);
  for (const edge of edges) {
    if (isForwardStep(step.id, edge.target, order)) {
      raiseRow(rows, edge.target, sourceRow + 1);
    }
  }
}

function routingRow(step, rows) {
  const memberRows = (step.fields?.members ?? []).map((member) => rows.get(member) ?? 0);
  return Math.max(rows.get(step.id) ?? 0, ...memberRows);
}

function isForwardStep(source, target, order) {
  return order.has(target) && order.get(target) > order.get(source);
}

function raiseRow(rows, id, row) {
  rows.set(id, Math.max(rows.get(id) ?? 0, row));
}

function placeSteps(steps, rows) {
  const rowCounts = new Map();
  return steps.map((step) => {
    const row = rows.get(step.id) ?? 0;
    const column = rowCounts.get(row) ?? 0;
    rowCounts.set(row, column + 1);
    return { ...step, row, column, x: column * X_GAP, y: row * Y_GAP };
  });
}

function placeTerminals(view, steps) {
  const railX = terminalX(steps);
  const rowByStep = new Map(steps.map((step) => [step.id, step.row]));
  return (view.terminals ?? []).map((terminal, index) => {
    const row = terminalRow(terminal.id, view.edges ?? [], rowByStep, index);
    return { ...terminal, row, column: "terminal", x: railX, y: row * Y_GAP };
  });
}

function terminalX(steps) {
  const maxColumn = Math.max(0, ...steps.map((step) => step.column));
  return (maxColumn + 1) * X_GAP + TERMINAL_GAP;
}

function terminalRow(id, edges, rowByStep, fallback) {
  const rows = edges
    .filter((edge) => edge.relation === "control" && edge.target === id)
    .map((edge) => rowByStep.get(edge.source))
    .filter((row) => row != null);
  return rows.length === 0 ? fallback : Math.max(...rows) + 1;
}

function placePipelineEdges(view, steps, terminals) {
  const positions = positionMap([...steps, ...terminals]);
  return [...(view.edges ?? [])]
    .sort(edgeSort)
    .map((edge) => ({ ...edge, route: routeOf(edge, positions) }));
}

function positionMap(items) {
  return new Map(items.map((item) => [item.id, item]));
}

function edgeSort(left, right) {
  return left.source.localeCompare(right.source) || edgeRank(left) - edgeRank(right) || left.id.localeCompare(right.id);
}

function edgeRank(edge) {
  return (RELATION_ORDER[edge.relation] ?? 9) * 10 + outcomeRank(edge.outcome);
}

function outcomeRank(outcome) {
  return ["success", "failure", "blocked", "no-work"].indexOf(outcome ?? "") + 1;
}

function routeOf(edge, positions) {
  if (ROUTES[edge.relation]) {
    return ROUTES[edge.relation];
  }
  const source = positions.get(edge.source);
  const target = positions.get(edge.target);
  if (!source || !target || target.y <= source.y) {
    return "back";
  }
  return edge.outcome === "success" ? "spine" : "exit";
}

function configMainItems(view) {
  const orphans = orphanSet(view);
  return [
    ...workSourceItems(view.assignments ?? []),
    ...kindItems("assignment", view.assignments ?? [], orphans),
    ...kindItems("role", view.roles ?? [], orphans),
    ...kindItems("repo", view.repos ?? [], orphans),
    ...kindItems("pipeline", view.pipelines ?? [], orphans),
  ];
}

function orphanSet(view) {
  return new Set((view.orphans ?? []).map((orphan) => `${orphan.kind}:${orphan.name}`));
}

function workSourceItems(assignments) {
  return assignments.map((assignment, row) => ({
    id: `work-source:${assignment.name}`,
    kind: "work-source",
    name: assignment.work.source,
    row,
  }));
}

function kindItems(kind, values, orphans = new Set()) {
  return values
    .filter((value) => !orphans.has(`${kind}:${value.name}`))
    .map((value, row) => ({ id: `${kind}:${value.name}`, kind, name: value.name, row }));
}

function configOrphanItems(view, mainCount) {
  const row = mainCount + 1;
  return (view.orphans ?? []).map((orphan, index) => ({
    id: `${orphan.kind}:${orphan.name}`,
    kind: orphan.kind,
    name: orphan.name,
    row: row + index,
    orphan: true,
  }));
}

function placeConfigItem(item) {
  const column = CONFIG_COLUMNS[item.kind] ?? 0;
  return { ...item, column, x: column * X_GAP, y: item.row * Y_GAP };
}

function configEdges(view, ids) {
  return (view.assignments ?? []).flatMap((assignment) => assignmentEdges(assignment, ids));
}

function assignmentEdges(assignment, ids) {
  return [
    configEdge("work", `work-source:${assignment.name}`, `assignment:${assignment.name}`),
    configEdge("role", `assignment:${assignment.name}`, `role:${assignment.role}`),
    configEdge("pipeline", `assignment:${assignment.name}`, `pipeline:${assignment.pipeline}`),
    ...assignment.repos.map((repo) => configEdge("repo", `assignment:${assignment.name}`, `repo:${repo}`)),
  ].filter((edge) => ids.has(edge.source) && ids.has(edge.target));
}

function configEdge(relation, source, target) {
  return { id: `${relation}:${source}->${target}`, source, target, relation };
}

function mapPlacedItems(layout, fn) {
  const copy = { ...layout };
  for (const key of ["steps", "terminals", "items"]) {
    if (copy[key]) {
      copy[key] = copy[key].map(fn);
    }
  }
  return copy;
}

function moveItem(item, position) {
  if (!validPosition(position)) {
    return item;
  }
  return { ...item, x: position.x, y: position.y };
}

function validPosition(position) {
  return Number.isFinite(position?.x) && Number.isFinite(position?.y);
}

function hashText(text) {
  let hash = 0x811c9dc5;
  for (const char of text) {
    hash ^= char.codePointAt(0);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}