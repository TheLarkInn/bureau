const X_GAP = 320;
const Y_GAP = 190;
// Config card geometry, mirrored by the CSS so a reserved box always holds its
// content. Heights are derived per card rather than fixed (see stackConfigColumns).
const CARD_BASE = 92;
const CHIP_ROW = 32;
const ACTION_ROW = 36;
const CONFIG_GAP = 28;
const CHIPS_PER_ROW = 2;
const DELETABLE = ["repo", "role", "assignment", "pipeline"];
const TERMINAL_GAP = 120;
const CONFIG_COLUMNS = {
  assignment: 0,
  pipeline: 1,
  role: 2,
  repo: 3,
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
  const main = stackConfigColumns(configMainItems(view), view, new Map());
  // Orphans sit below every column, not merely below their own, so "detached"
  // reads as detached however tall the other columns happen to be.
  const baseline = main.reduce((lowest, item) => Math.max(lowest, item.y + item.height), 0) + CONFIG_GAP * 2;
  const orphans = stackConfigColumns(configOrphanItems(view, main.length), view, startingAt(baseline));
  const items = [...main, ...orphans];
  const edges = configEdges(view, new Set(items.map((item) => item.id)));
  return mergeArrangement({ dir: view.dir, items, edges }, arrangement);
}

function startingAt(baseline) {
  return new Map(Object.values(CONFIG_COLUMNS).map((column) => [column, baseline]));
}

/**
 * Places config cards by stacking each column with running offsets, using a
 * height derived from the card's own content.
 *
 * A fixed row gap cannot work here: a role with eleven permissions is far
 * taller than one with two, and a pipeline lists a line per agent step. Tuning
 * one global constant just moves which config overlaps.
 */
function stackConfigColumns(items, view, offsets) {
  return items.map((item) => {
    const column = CONFIG_COLUMNS[item.kind] ?? 0;
    const height = configCardHeight(item, view);
    const y = offsets.get(column) ?? 0;
    offsets.set(column, y + height + CONFIG_GAP);
    return { ...item, column, height, x: column * X_GAP, y };
  });
}

/** Mirrors what the card renders, so the DOM cannot exceed the reserved box. */
function configCardHeight(item, view) {
  return CARD_BASE + chipRowsFor(item, view) * CHIP_ROW + (DELETABLE.includes(item.kind) ? ACTION_ROW : 0);
}

function chipRowsFor(item, view) {
  const record = configRecord(item, view);
  if (item.kind === "role") {
    return Math.ceil((record?.permissions?.length ?? 0) / CHIPS_PER_ROW);
  }
  if (item.kind === "pipeline") {
    return 1 + (record?.stepCount ?? 0);
  }
  if (item.kind === "assignment") {
    return Math.ceil(countLimits(record) / CHIPS_PER_ROW);
  }
  return 1;
}

function countLimits(record) {
  return Object.values(record?.limits ?? {}).filter((value) => value != null).length;
}

function configRecord(item, view) {
  const collection = { role: "roles", repo: "repos", assignment: "assignments", pipeline: "pipelines" }[item.kind];
  return collection ? (view[collection] ?? []).find((value) => value.name === item.name) : null;
}

export function pipelineHandles(layout) {
  const items = [...(layout.steps ?? []), ...(layout.terminals ?? [])];
  const handles = {
    items: Object.fromEntries(items.map((item) => [item.id, { source: [], target: [] }])),
    edges: {},
  };
  for (const edge of layout.edges ?? []) {
    const pair = handlePair(edge);
    addHandle(handles.items, edge.source, "source", pair.source);
    addHandle(handles.items, edge.target, "target", pair.target);
    handles.edges[edge.id] = { source: pair.source.id, target: pair.target.id };
  }
  return sortHandles(handles);
}

export function pipelineContainers(layout) {
  const steps = positionMap(layout.steps ?? []);
  return (layout.steps ?? [])
    .filter((step) => step.kind === "concurrent")
    .map((step) => containerFor(step, steps));
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

function handlePair(edge) {
  if (edge.route === "spine") {
    return { source: handle("bottom", edge.outcome), target: handle("top", "in") };
  }
  if (edge.route === "back") {
    return { source: handle("left", "loop"), target: handle("left", "in-left") };
  }
  if (edge.route === "data" || edge.route === "observes") {
    return { source: handle("right", edge.route), target: handle("left", "in-left") };
  }
  return { source: handle("right", edge.outcome), target: handle("top", "in") };
}

function handle(side, name) {
  return { id: `${side}:${name}`, side, name };
}

function addHandle(items, id, direction, handle) {
  const item = items[id];
  if (!item) {
    return;
  }
  if (!item[direction].some((existing) => existing.id === handle.id)) {
    item[direction].push(handle);
  }
}

function sortHandles(handles) {
  return {
    items: Object.fromEntries(Object.entries(handles.items).map(([id, item]) => [id, {
      source: item.source.sort(handleSort),
      target: item.target.sort(handleSort),
    }])),
    edges: handles.edges,
  };
}

function handleSort(left, right) {
  return sideRank(left.side) - sideRank(right.side) || left.name.localeCompare(right.name);
}

function sideRank(side) {
  return ["top", "right", "bottom", "left"].indexOf(side);
}

function containerFor(step, steps) {
  const members = (step.fields.members ?? []).map((member) => steps.get(member)).filter(Boolean);
  const boxed = [step, ...members];
  const left = Math.min(...boxed.map((item) => item.x));
  const top = Math.min(...boxed.map((item) => item.y));
  const right = Math.max(...boxed.map((item) => item.x));
  const bottom = Math.max(...boxed.map((item) => item.y));
  return {
    id: `concurrent:${step.id}`,
    parent: step.id,
    members: members.map((member) => member.id),
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  };
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
  const occupied = new Set();
  return (view.terminals ?? []).map((terminal, index) => {
    let row = terminalRow(terminal.id, view.edges ?? [], rowByStep, index);
    while (occupied.has(row)) {
      row += 1;
    }
    occupied.add(row);
    return { ...terminal, row, column: "terminal", x: railX, y: row * Y_GAP };
  });
}

/**
 * Where the terminal rail stands: clear of the widest row, whatever widened it.
 *
 * This used to measure only the steps on the rail (`parentId == null`), on the
 * reasoning that a concurrent group's members belong to their group rather than
 * to the spine. They are placed in the same coordinates as everything else,
 * though — `column * X_GAP`, from the same per-row counter — so a group with two
 * members puts a card in column 1 while the rail still measured one column, and
 * the terminals came to rest 120px inside it. A terminal pill printed over a
 * member card, in the one shape of pipeline the bundled sample does not have.
 */
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
  return assignColumnRows([
    ...kindItems("assignment", view.assignments ?? [], orphans),
    ...kindItems("pipeline", view.pipelines ?? [], orphans),
    ...kindItems("role", view.roles ?? [], orphans),
    ...kindItems("repo", view.repos ?? [], orphans),
  ]);
}

function assignColumnRows(items) {
  const rows = new Map();
  return items.map((item) => {
    const column = CONFIG_COLUMNS[item.kind] ?? 0;
    const row = rows.get(column) ?? 0;
    rows.set(column, row + 1);
    return { ...item, row };
  });
}

function orphanSet(view) {
  return new Set((view.orphans ?? []).map((orphan) => `${orphan.kind}:${orphan.name}`));
}

function kindItems(kind, values, orphans = new Set()) {
  return values
    .filter((value) => !orphans.has(`${kind}:${value.name}`))
    .map((value) => ({ id: `${kind}:${value.name}`, kind, name: value.name }));
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

function configEdges(view, ids) {
  return [
    ...(view.assignments ?? []).flatMap((assignment) => assignmentEdges(assignment, ids)),
    ...(view.pipelines ?? []).flatMap((pipeline) => pipelineEdges(pipeline, ids)),
  ];
}

function assignmentEdges(assignment, ids) {
  return [
    configEdge("pipeline", `assignment:${assignment.name}`, `pipeline:${assignment.pipeline}`),
    ...assignment.repos.map((repo) => configEdge("repo", `assignment:${assignment.name}`, `repo:${repo}`)),
  ].filter((edge) => ids.has(edge.source) && ids.has(edge.target));
}

function pipelineEdges(pipeline, ids) {
  return (pipeline.roles ?? [])
    .map((role) => configEdge("role", `pipeline:${pipeline.name}`, `role:${role}`))
    .filter((edge) => ids.has(edge.source) && ids.has(edge.target));
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