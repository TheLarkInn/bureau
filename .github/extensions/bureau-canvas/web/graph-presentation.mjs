import { layoutPipeline } from "./layout.js";

export const GRAPH_STATE_LABELS = {
  design: "Design",
  unknown: "No run",
  pending: "Pending",
  running: "Running",
  paused: "Paused",
  success: "Success",
  failure: "Failed",
  blocked: "Blocked",
  "no-work": "No work",
  skipped: "Skipped",
  cancelled: "Cancelled",
};

export function graphStepState(node, mode = "design") {
  if (mode === "design") {
    return "design";
  }
  if (node?.paused) {
    return "paused";
  }
  const state = node?.className?.replace(/^overlay-/u, "");
  return Object.hasOwn(GRAPH_STATE_LABELS, state) ? state : "unknown";
}

export function needsAttention(state, findings = []) {
  return ["failure", "blocked", "paused"].includes(state) || findings.length > 0;
}

export function searchGraphItems(items, query) {
  const text = query.trim().toLocaleLowerCase();
  return items.map((item, index) => ({ item, index, rank: matchRank(item, text) }))
    .filter(({ rank }) => rank < 4)
    .sort((left, right) => left.rank - right.rank || left.index - right.index)
    .map(({ item }) => item);
}

function matchRank(item, text) {
  const name = item.name.toLocaleLowerCase();
  if (!text || name.startsWith(text)) {
    return 0;
  }
  if (name.includes(text)) {
    return 1;
  }
  const context = [item.kind, item.detail, item.state, GRAPH_STATE_LABELS[item.state]].join(" ").toLocaleLowerCase();
  return context.includes(text) ? 2 : 4;
}

export function nextAttention(items, selectedId) {
  const attention = items.filter((item) => item.attention);
  const current = attention.findIndex((item) => item.id === selectedId);
  return attention[(current + 1) % attention.length] ?? null;
}

export function initialGraphViewport(bounds, width, height) {
  const zoom = Math.max(0.8, Math.min(1,
    (width - 64) / Math.max(1, bounds.width),
    (height - 168) / Math.max(1, bounds.height)));
  return {
    x: Math.max(32, (width - bounds.width * zoom) / 2) - bounds.x * zoom,
    y: Math.max(72, (height - bounds.height * zoom) / 2) - bounds.y * zoom,
    zoom,
  };
}

// Shared with the editor: terminal labels form their own rail rather than
// following every long exit curve through the intervening step cards.
export function graphEdgeLabels(edges) {
  const groups = new Map();
  const terminals = [...new Set(edges.filter((edge) => edge.target.startsWith("terminal:"))
    .map((edge) => edge.target))].sort();
  const keyFor = (edge) => edge.target.startsWith("terminal:") ? edge.target : `${edge.source}->${edge.target}`;
  for (const edge of edges) {
    const key = keyFor(edge);
    groups.set(key, [...(groups.get(key) ?? []), edge]);
  }
  return edges.map((edge) => {
    const siblings = groups.get(keyFor(edge));
    return { ...edge, data: { ...edge.data,
      labelOffset: (siblings.indexOf(edge) - (siblings.length - 1) / 2) * 28,
      terminalLabel: edge.target.startsWith("terminal:"),
      terminalColumn: terminals.indexOf(edge.target),
    } };
  });
}

export function graphEdgeCaption({ data, labelX, labelY, targetX, targetY }) {
  return [
    data?.terminalLabel ? targetX - 72 - data.terminalColumn * 56 : labelX,
    (data?.terminalLabel ? targetY : labelY) + (data?.labelOffset ?? 0),
  ];
}

// Pass each exit curve through its caption rail, so repeated outcome labels
// remain attached to their own handoff rather than floating beside a bundle.
export function graphTerminalPath({ sourceX, sourceY, targetX, targetY }, captionX, captionY) {
  const entry = Math.abs(captionX - sourceX) / 2;
  const exit = Math.abs(targetX - captionX) / 2;
  return `M${sourceX},${sourceY} C${sourceX + entry},${sourceY} ${captionX - entry},${captionY} ${captionX},${captionY}`
    + ` C${captionX + exit},${captionY} ${targetX - exit},${targetY} ${targetX},${targetY}`;
}

// Viewer and editor use the same left-to-right placement; saved coordinates
// remain authoritative and run decorations never move a step.
export function graphGeometry(pipeline) {
  const layout = pipeline?.layout ?? { steps: [], terminals: [], edges: [] };
  const view = pipeline?.view ?? layout;
  const placed = layoutPipeline(view);
  const positions = new Map(placed.nodes.map((node) => [node.id, node]));
  const move = (item) => {
    const position = pipeline?.arrangement?.[item.id] ?? positions.get(item.id) ?? item;
    return { ...item, x: position.x, y: position.y };
  };
  const steps = layout.steps.map(move);
  const terminals = layout.terminals.map(move);
  const byId = new Map(steps.map((step) => [step.id, step]));
  const containers = (pipeline?.containers ?? []).map((frame) => frameGeometry(frame, byId));
  return { ...layout, steps, terminals, containers };
}

function frameGeometry(frame, byId) {
  const members = [frame.parent, ...frame.members].map((id) => byId.get(id)).filter(Boolean);
  if (!members.length) {
    return frame;
  }
  const x = Math.min(...members.map((step) => step.x));
  const y = Math.min(...members.map((step) => step.y));
  return { ...frame, x, y, width: Math.max(...members.map((step) => step.x)) - x,
    height: Math.max(...members.map((step) => step.y)) - y };
}
