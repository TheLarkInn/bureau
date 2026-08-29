// The config relation graph (Q16): assignments point at pipelines and repos;
// pipelines point at the roles their steps name, whatever the step's kind —
// the same reference `lib/preflight.mjs` counts. Editing happens in the
// pipeline editor and the existing forms.

import React from "react";
import { Background, Controls, Handle, MiniMap, Position, ReactFlow } from "@xyflow/react";

import { drawableEdges } from "../graph-edges.mjs";
import { MeasurementGuard } from "../graph-measure.mjs";

const h = React.createElement;
const NODE_WIDTH = 240;
const NODE_HEIGHT = 96;
const X_GAP = 300;
const Y_GAP = 130;
const COLUMNS = { assignment: 0, pipeline: 1, role: 2, repo: 3 };

export function RelationGraph({ relation }) {
  // Counted from the config, never from `flow`: see `web/graph-edges.mjs`.
  const source = relation ?? { nodes: [], edges: [] };
  const flow = toFlow(source);
  return h(
    "div",
    { className: "relation-flow", "aria-label": "Config relation graph", "data-graph-edges": String(drawableEdges(source.nodes, source.edges)) },
    h(
      ReactFlow,
      {
        nodes: flow.nodes,
        edges: flow.edges,
        nodeTypes: { relationCard: RelationCard },
        fitView: true,
        fitViewOptions: { padding: 0.18 },
        minZoom: 0.2,
        maxZoom: 1.5,
        nodesDraggable: false,
        nodesConnectable: false,
        proOptions: { hideAttribution: true },
      },
      h(Background, { gap: 24, size: 1.5 }),
      h(Controls),
      h(MiniMap, { pannable: true, zoomable: true }),
      h(MeasurementGuard, { ids: flow.nodes.map((node) => node.id) }),
    ),
  );
}

function toFlow(relation) {
  const placed = place(relation.nodes);
  return {
    nodes: placed.map((node) => ({
      id: node.id,
      type: "relationCard",
      position: { x: node.x, y: node.y },
      data: { node },
      draggable: false,
      connectable: false,
      style: { width: NODE_WIDTH, height: NODE_HEIGHT },
    })),
    edges: relation.edges.map((edge) => ({
      id: edge.id,
      source: edge.source,
      target: edge.target,
      type: "smoothstep",
      focusable: false,
      selectable: false,
      label: edge.relation,
      style: { stroke: "var(--border-color-default, #d0d7de)", strokeWidth: 1.4 },
    })),
  };
}

function place(nodes) {
  const rows = new Map();
  return [...nodes]
    .sort((left, right) => (COLUMNS[left.kind] ?? 0) - (COLUMNS[right.kind] ?? 0) || left.name.localeCompare(right.name))
    .map((node) => {
      const column = COLUMNS[node.kind] ?? 0;
      const row = rows.get(column) ?? 0;
      rows.set(column, row + 1);
      return { ...node, x: column * X_GAP, y: row * Y_GAP };
    });
}

function RelationCard({ data }) {
  const node = data.node;
  return h(
    "article",
    { className: `relation-card relation-card--${node.kind}`, "data-ref": node.id },
    h(Handle, { id: "in", type: "target", position: Position.Left }),
    h(Handle, { id: "out", type: "source", position: Position.Right }),
    h("p", { className: "kind-label" }, node.kind),
    h("h2", {}, node.name),
  );
}
