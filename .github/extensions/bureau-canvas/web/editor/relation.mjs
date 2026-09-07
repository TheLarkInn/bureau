// The config relation graph (Q16): assignments point at pipelines and repos;
// pipelines point at the roles their steps name, whatever the step's kind —
// the same reference `lib/preflight.mjs` counts. Editing happens in the
// pipeline editor and the existing forms.

import React, { useMemo, useState } from "react";
import {
  Background, BackgroundVariant, Handle, MiniMap, Position, ReactFlow,
} from "@xyflow/react";

import { drawableEdges } from "../graph-edges.mjs";
import { MeasurementGuard } from "../graph-measure.mjs";
import { GraphTools, GraphStateBadge } from "../graph-workbench.mjs";

const h = React.createElement;
const NODE_WIDTH = 240;
const NODE_HEIGHT = 106;
const X_GAP = 300;
const Y_GAP = 130;
const COLUMNS = { assignment: 0, pipeline: 1, role: 2, repo: 3 };
const NODE_TYPES = { relationCard: RelationCard };

export function RelationGraph({ relation }) {
  const [selected, setSelected] = useState(null);
  // Counted from the config, never from `flow`: see `web/graph-edges.mjs`.
  const source = relation ?? { nodes: [], edges: [] };
  const flow = useMemo(() => toFlow(relation ?? { nodes: [], edges: [] }), [relation]);
  return h(
    "div",
    { className: "relation-flow", "aria-label": "Config relation graph", "data-graph-edges": String(drawableEdges(source.nodes, source.edges)) },
    h(
      ReactFlow,
      {
        nodes: flow.nodes.map((node) => ({ ...node, selected: node.id === selected })),
        edges: flow.edges,
        nodeTypes: NODE_TYPES,
        minZoom: 0.2,
        maxZoom: 3,
        nodesDraggable: false,
        nodesConnectable: false,
        onNodeClick: (_, node) => setSelected(node.id),
        onNodesChange: (changes) => {
          const change = changes.find((item) => item.type === "select" && item.selected);
          if (change) {
            setSelected(change.id);
          }
        },
        proOptions: { hideAttribution: true },
      },
      h(Background, { variant: BackgroundVariant.Lines, gap: 48, size: 1 }),
      h(GraphTools, {
        items: source.nodes.map((node) => ({ ...node, detail: node.id, state: "design" })),
        selectedId: selected,
        onSelect: setSelected,
        label: "nodes",
      }),
      h(MiniMap, { position: "bottom-left", pannable: true, zoomable: true, "aria-label": "Config relation overview" }),
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
      style: { width: NODE_WIDTH, minHeight: NODE_HEIGHT },
    })),
    edges: relation.edges.map((edge) => ({
      id: edge.id,
      source: edge.source,
      target: edge.target,
      type: "default",
      focusable: false,
      selectable: false,
      label: edge.relation,
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
    h("div", { className: "graph-card-heading" },
      h("h3", { title: node.name }, node.name),
      h(GraphStateBadge, { state: "design" })),
    h("div", { className: "graph-card-meta" },
      h("p", { className: "kind-label" }, node.kind)),
  );
}
