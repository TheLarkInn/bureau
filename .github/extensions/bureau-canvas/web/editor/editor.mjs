// The pipeline editor: full node/edge CRUD on one pipeline's step graph.
//
// The editor keeps a draft pipeline view in React state, applies edits
// through the pure transforms in `../editor/…` (`lib/edit.mjs` semantics
// mirrored client-side against the same view shape), and saves through the
// server's `save-pipeline` intent, which owns validation and the revert.
// Node positions ride along as the layout sidecar (Q10).

import React, { useMemo, useState } from "react";
import {
  Background,
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  getSmoothStepPath,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
} from "@xyflow/react";

import { layoutPipeline } from "../layout.js";

const h = React.createElement;
const OUTCOMES = ["success", "failure", "blocked", "no-work"];
const TERMINALS = ["done", "abort", "escalate"];
const STEP_KINDS = ["deterministic", "agent", "decision", "concurrent"];
const CONTROL_FIELDS = [
  ["next", "success"],
  ["on_failure", "failure"],
  ["on_blocked", "blocked"],
  ["on_no_work", "no-work"],
];
const NODE_TYPES = { stepNode: StepNode, terminalNode: TerminalNode };
const EDGE_TYPES = { outcome: OutcomeEdge };

export function PipelineEditor({ state, name, onSaved }) {
  const pipeline = state.pipelines?.[name];
  const [draft, setDraft] = useState(null);
  const [positions, setPositions] = useState(() => ({ ...(pipeline?.arrangement ?? {}) }));
  const [selected, setSelected] = useState(null);
  const [saveResult, setSaveResult] = useState(null);

  const view = useMemo(() => draft ?? editableView(pipeline?.view), [draft, pipeline]);
  const hints = useMemo(() => problems(view), [view]);
  const flow = useMemo(() => toFlow(view, positions, hints, saveResult), [view, positions, hints, saveResult]);

  const edit = (next) => {
    setDraft(next);
    setSaveResult(null);
  };
  const selectedStep = view.steps.find((step) => step.name === selected) ?? null;

  return h(
    "section",
    { className: "editor-shell" },
    h(EditorToolbar, { name, view, dirty: draft != null, hints, saveResult, onSave: () => save(setSaveResult, onSaved, name, view, positions), onAdd: (kind) => addStep(name, view, kind, edit, setSelected) }),
    h(
      "div",
      { className: "editor-main" },
      h(
        "div",
        { className: "editor-flow" },
        h(
          ReactFlow,
          {
            nodes: flow.nodes,
            edges: flow.edges,
            nodeTypes: NODE_TYPES,
            edgeTypes: EDGE_TYPES,
            fitView: true,
            fitViewOptions: { padding: 0.22 },
            minZoom: 0.2,
            maxZoom: 1.5,
            nodesDraggable: true,
            nodesConnectable: true,
            deleteKeyCode: null,
            proOptions: { hideAttribution: true },
            onNodeClick: (_, node) => setSelected(node.type === "stepNode" ? node.id : null),
            onNodeDragStop: (_, node) => setPositions((current) => ({ ...current, [node.id]: { x: Math.round(node.position.x), y: Math.round(node.position.y) } })),
            onConnect: (connection) => connect(view, connection, edit),
            onEdgeClick: (_, edge) => edge.data?.editable && edit(setEdge(view, edge.data.sourceStep, edge.data.outcome, null)),
          },
          h(Background, { gap: 24, size: 1.5 }),
          h(Controls),
          h(MiniMap, { pannable: true, zoomable: true }),
        ),
      ),
      h(SidePanel, {
        view,
        step: selectedStep,
        hints,
        saveResult,
        onChange: (next) => edit(next),
        onClose: () => setSelected(null),
        onDelete: selectedStep ? () => {
          edit(removeStep(view, selectedStep.name));
          setSelected(null);
        } : null,
      }),
    ),
  );
}

/** The server's pipeline view, lifted into editable form (per-step outgoing). */
function editableView(view) {
  const steps = (view?.steps ?? []).map((step) => ({
    ...step,
    outgoing: (view?.edges ?? []).filter((edge) => edge.relation === "control" && edge.source === step.name),
  }));
  return { name: view?.name ?? "", steps, terminals: view?.terminals ?? [], edges: view?.edges ?? [] };
}

// --- draft transforms (same semantics as lib/edit.mjs) ---

function setEdge(view, source, outcome, target) {
  const others = view.edges.filter((edge) => !(edge.relation === "control" && edge.source === source && edge.outcome === outcome));
  const steps = view.steps.map((step) => (step.name === source ? { ...step, outgoing: step.outgoing.filter((edge) => edge.outcome !== outcome) } : step));
  if (target == null) {
    return syncSteps({ ...view, steps, edges: others });
  }
  const resolved = TERMINALS.includes(target) ? `terminal:${target}` : target;
  const edge = { id: `control:${source}:${outcome}->${resolved}`, source, target: resolved, relation: "control", outcome };
  return syncSteps({ ...view, steps, edges: [...others, edge] });
}

function syncSteps(view) {
  return {
    ...view,
    steps: view.steps.map((step) => ({ ...step, outgoing: view.edges.filter((edge) => edge.relation === "control" && edge.source === step.name) })),
  };
}

function addStep(name, view, kind, edit, setSelected) {
  const base = `step-${view.steps.length + 1}`;
  let candidate = base;
  for (let suffix = 2; view.steps.some((step) => step.name === candidate); suffix += 1) {
    candidate = `${base}-${suffix}`;
  }
  const fields = { inputsFrom: [], maxAttempts: 1 };
  if (kind === "deterministic") {
    fields.run = "true";
  }
  if (kind === "decision") {
    fields.over = view.steps[0]?.name ?? "";
    fields.on = Object.fromEntries(OUTCOMES.map((outcome) => [outcome, "abort"]));
  }
  if (kind === "concurrent") {
    fields.members = [];
    fields.completion = "all";
  }
  edit({ ...view, steps: [...view.steps, { id: candidate, name: candidate, type: "step", kind, order: view.steps.length, fields, outgoing: [] }] });
  setSelected(candidate);
}

function removeStep(view, stepName) {
  return {
    ...view,
    steps: view.steps.filter((step) => step.name !== stepName),
    edges: view.edges.filter((edge) => edge.source !== stepName && edge.target !== stepName),
  };
}

function connect(view, connection, edit) {
  if (!connection?.source || !connection.target) {
    return;
  }
  const outcome = outcomeFromHandle(connection.sourceHandle);
  edit(setEdge(view, connection.source, outcome, connection.target));
}

function outcomeFromHandle(handle) {
  const name = String(handle ?? "").split(":").at(-1);
  return OUTCOMES.includes(name) ? name : "success";
}

// --- layout + flow projection ---

function toFlow(view, positions, hints, saveResult) {
  const auto = layoutPipeline(view);
  const marked = new Map();
  for (const hint of hints) {
    marked.set(hint.step, [...(marked.get(hint.step) ?? []), hint.message]);
  }
  for (const finding of saveResult?.findings ?? []) {
    const target = finding.target ?? {};
    if (target.kind === "step" && target.step) {
      marked.set(target.step, [...(marked.get(target.step) ?? []), finding.message]);
    }
  }
  return {
    nodes: auto.nodes.map((node) => flowNode(node, positions, marked)),
    edges: flowEdges(view),
  };
}

function flowNode(node, positions, marked) {
  const saved = positions[node.id];
  const position = saved && Number.isFinite(saved.x) && Number.isFinite(saved.y) ? saved : { x: node.x, y: node.y };
  if (!node.step) {
    return { id: node.id, type: "terminalNode", position, data: { name: node.name }, draggable: true };
  }
  return {
    id: node.id,
    type: "stepNode",
    position,
    data: { step: node.step, hints: marked.get(node.id) ?? [] },
    draggable: true,
  };
}

function flowEdges(view) {
  return view.edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    sourceHandle: `out:${edge.outcome ?? edge.relation}`,
    targetHandle: "in",
    type: "outcome",
    label: edge.relation === "control" ? edge.outcome : edge.relation,
    className: `editor-edge--${edge.relation === "control" ? edge.outcome : edge.relation}`,
    markerEnd: { type: MarkerType.ArrowClosed },
    data: edge.relation === "control" ? { editable: true, sourceStep: edge.source, outcome: edge.outcome } : { editable: false },
  }));
}

// --- nodes ---

function StepNode({ data, selected }) {
  const step = data.step;
  const classes = ["editor-card", `editor-card--${step.kind}`, data.hints.length ? "editor-card--hint" : "", selected ? "is-highlighted" : ""].filter(Boolean).join(" ");
  return h(
    "article",
    { className: classes, "data-ref": step.name },
    h(Handle, { id: "in", type: "target", position: Position.Top, className: "editor-handle" }),
    OUTCOMES.map((outcome) =>
      h(Handle, {
        key: outcome,
        id: `out:${outcome}`,
        type: "source",
        position: Position.Bottom,
        className: `editor-handle editor-handle--${outcome}`,
        style: { left: `${10 + OUTCOMES.indexOf(outcome) * 26}%` },
        title: outcome,
      }),
    ),
    h("p", { className: "kind-label" }, step.kind),
    h("h2", {}, step.name),
    h("p", { className: "detail" }, stepDetail(step)),
    data.hints.length ? h("ul", { className: "editor-hints" }, data.hints.map((message) => h("li", { key: message }, message))) : null,
  );
}

function stepDetail(step) {
  if (step.kind === "deterministic") {
    return step.fields.run ?? "run command not set";
  }
  if (step.kind === "agent") {
    return `role: ${step.fields.role ?? "not set"}`;
  }
  if (step.kind === "decision") {
    return `over: ${step.fields.over ?? "not set"}`;
  }
  return `${step.fields.members?.length ?? 0} members`;
}

function TerminalNode({ data }) {
  return h(
    "article",
    { className: `editor-terminal editor-terminal--${data.name}` },
    h(Handle, { id: "in", type: "target", position: Position.Top, className: "editor-handle" }),
    h("h2", {}, data.name),
  );
}

function OutcomeEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, label }) {
  const [path, labelX, labelY] = getSmoothStepPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition, offset: 16 });
  return h(
    React.Fragment,
    null,
    h(BaseEdge, { id, path, markerEnd }),
    label
      ? h(EdgeLabelRenderer, null, h("div", { className: "react-flow__edge-label edge-caption", style: { transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` } }, label))
      : null,
  );
}

// --- toolbar + side panel ---

function EditorToolbar({ name, dirty, hints, saveResult, onSave, onAdd }) {
  const [kind, setKind] = useState("deterministic");
  return h(
    "div",
    { className: "editor-toolbar" },
    h("h2", {}, name),
    h(
      "span",
      { className: "editor-toolbar-add" },
      h(
        "select",
        { value: kind, onChange: (event) => setKind(event.target.value), "aria-label": "New step kind" },
        STEP_KINDS.map((option) => h("option", { key: option, value: option }, option)),
      ),
      h("button", { type: "button", onClick: () => onAdd(kind) }, "Add step"),
    ),
    h("span", { className: `editor-status${hints.length ? " editor-status--hints" : ""}` }, statusText(dirty, hints, saveResult)),
    h("button", { type: "button", className: "editor-save", disabled: !dirty, onClick: onSave }, "Save"),
  );
}

function statusText(dirty, hints, saveResult) {
  if (saveResult && !saveResult.ok) {
    return "save reverted — see findings";
  }
  if (hints.length) {
    return `${hints.length} hint${hints.length === 1 ? "" : "s"}`;
  }
  return dirty ? "unsaved edits" : "saved";
}

function SidePanel({ view, step, hints, saveResult, onChange, onClose, onDelete }) {
  return h(
    "aside",
    { className: "editor-panel" },
    step
      ? h(StepEditor, { view, step, onChange, onClose, onDelete })
      : h("p", { className: "muted" }, "Select a step to edit it. Drag between a bottom outcome handle and another node to rewire."),
    hints.length
      ? h("section", { className: "panel-section" }, h("h3", {}, `Hints (${hints.length})`), h("ul", { className: "editor-hints" }, hints.map((hint) => h("li", { key: `${hint.step}:${hint.message}` }, `${hint.step}: ${hint.message}`))))
      : null,
    saveResult && !saveResult.ok
      ? h("section", { className: "panel-section" }, h("h3", {}, "Save reverted"), h("ul", { className: "editor-hints" }, (saveResult.findings ?? []).map((finding, index) => h("li", { key: index }, finding.message))))
      : null,
  );
}

function StepEditor({ view, step, onChange, onClose, onDelete }) {
  const set = (field, value) => onChange(setField(view, step.name, field, value));
  return h(
    "section",
    { className: "panel-section editor-step" },
    h("div", { className: "editor-step-head" }, h("h3", {}, step.name), h("button", { type: "button", onClick: onClose }, "Close")),
    h("label", {}, "name", h("input", { value: step.name, onChange: (event) => onChange(renameStep(view, step.name, event.target.value)) })),
    h(KindFields, { step, set }),
    h(EdgeEditor, { view, step, onChange }),
    h("label", {}, "max attempts", h("input", { type: "number", min: 1, value: step.fields.maxAttempts ?? 1, onChange: (event) => set("maxAttempts", Number(event.target.value) || 1) })),
    h("button", { type: "button", className: "card-action", onClick: onDelete }, "Delete step"),
  );
}

function setField(view, name, field, value) {
  return {
    ...view,
    steps: view.steps.map((step) => (step.name === name ? { ...step, fields: { ...step.fields, [field]: value } } : step)),
  };
}

function renameStep(view, from, to) {
  if (!to || to === from || view.steps.some((step) => step.name === to)) {
    return view;
  }
  const steps = view.steps.map((step) => {
    const fields = { ...step.fields };
    if (fields.over === from) {
      fields.over = to;
    }
    if (fields.on) {
      fields.on = Object.fromEntries(Object.entries(fields.on).map(([outcome, target]) => [outcome, target === from ? to : target]));
    }
    if (Array.isArray(fields.members)) {
      fields.members = fields.members.map((member) => (member === from ? to : member));
    }
    if (Array.isArray(fields.inputsFrom)) {
      fields.inputsFrom = fields.inputsFrom.map((source) => (source === from ? to : source));
    }
    return { ...step, id: step.name === from ? to : step.id, name: step.name === from ? to : step.name, fields };
  });
  const edges = view.edges.map((edge) => ({
    ...edge,
    source: edge.source === from ? to : edge.source,
    target: edge.target === from ? to : edge.target,
  }));
  return syncSteps({ ...view, steps, edges });
}

function KindFields({ step, set }) {
  if (step.kind === "deterministic") {
    return h("label", {}, "run", h("textarea", { value: step.fields.run ?? "", onChange: (event) => set("run", event.target.value) }));
  }
  if (step.kind === "agent") {
    return h(React.Fragment, null,
      h("label", {}, "role", h("input", { value: step.fields.role ?? "", onChange: (event) => set("role", event.target.value) })),
      h("label", {}, "trust", h("input", { value: step.fields.trust ?? "", onChange: (event) => set("trust", event.target.value || null) })),
    );
  }
  if (step.kind === "decision") {
    return h("label", {}, "over", h("input", { value: step.fields.over ?? "", onChange: (event) => set("over", event.target.value) }));
  }
  return h(React.Fragment, null,
    h("label", {}, "members (comma-separated)", h("input", {
      value: (step.fields.members ?? []).join(", "),
      onChange: (event) => set("members", event.target.value.split(",").map((member) => member.trim()).filter(Boolean)),
    })),
  );
}

function EdgeEditor({ view, step, onChange }) {
  if (step.kind === "decision") {
    return h(DecisionEditor, { view, step, onChange });
  }
  return h(
    "fieldset",
    { className: "editor-edges" },
    h("legend", {}, "edges"),
    CONTROL_FIELDS.map(([field, outcome]) =>
      h(OutcomeSelect, { key: field, view, step, outcome, onChange }),
    ),
  );
}

function OutcomeSelect({ view, step, outcome, onChange }) {
  const current = edgeTarget(view, step.name, outcome);
  return h(
    "label",
    {},
    outcome,
    h(
      "select",
      { value: current ?? "", onChange: (event) => onChange(setEdge(view, step.name, outcome, event.target.value || null)) },
      h("option", { value: "" }, "abort (default)"),
      view.steps.filter((candidate) => candidate.name !== step.name).map((candidate) => h("option", { key: candidate.name, value: candidate.name }, candidate.name)),
      TERMINALS.map((terminal) => h("option", { key: terminal, value: terminal }, `terminal: ${terminal}`)),
    ),
  );
}

function DecisionEditor({ view, step, onChange }) {
  const on = step.fields.on ?? {};
  const gaps = decisionGaps(step);
  return h(
    "fieldset",
    { className: "editor-edges" },
    h("legend", {}, "on (all four outcomes required)"),
    OUTCOMES.map((outcome) =>
      h(
        "label",
        { key: outcome },
        outcome,
        h(
          "select",
          {
            value: on[outcome] ?? "",
            onChange: (event) => onChange(setField(view, step.name, "on", { ...on, [outcome]: event.target.value })),
          },
          h("option", { value: "" }, "—"),
          view.steps.filter((candidate) => candidate.name !== step.name).map((candidate) => h("option", { key: candidate.name, value: candidate.name }, candidate.name)),
          TERMINALS.map((terminal) => h("option", { key: terminal, value: terminal }, `terminal: ${terminal}`)),
        ),
      ),
    ),
    gaps.length ? h("p", { className: "editor-hints" }, gaps.join("; ")) : null,
  );
}

function edgeTarget(view, source, outcome) {
  const edge = view.edges.find((candidate) => candidate.relation === "control" && candidate.source === source && candidate.outcome === outcome);
  return edge ? plainTarget(edge.target) : null;
}

function plainTarget(target) {
  return target.startsWith("terminal:") ? target.slice("terminal:".length) : target;
}

// --- hints + save ---

function decisionGaps(step) {
  const on = step.fields?.on ?? {};
  const missing = OUTCOMES.filter((outcome) => !present(on[outcome]));
  const unknown = Object.keys(on).filter((outcome) => !OUTCOMES.includes(outcome) && present(on[outcome]));
  return [...missing.map((outcome) => `missing \`${outcome}\` branch`), ...unknown.map((outcome) => `unknown outcome \`${outcome}\``)];
}

function present(value) {
  return typeof value === "string" && value.length > 0;
}

function problems(view) {
  const reached = new Set(view.edges.filter((edge) => edge.relation === "control").map((edge) => edge.target));
  const known = new Set([...view.steps.map((step) => step.name), ...TERMINALS.map((name) => `terminal:${name}`)]);
  return [
    ...view.steps.filter((step, index) => index > 0 && !reached.has(step.name)).map((step) => ({ step: step.name, message: "step is not reachable by any control edge" })),
    ...view.edges
      .filter((edge) => edge.relation === "control" && !known.has(edge.target))
      .map((edge) => ({ step: edge.source, message: `\`${edge.outcome}\` edge points at \`${plainTarget(edge.target)}\`, which does not exist` })),
    ...view.steps.flatMap((step) => decisionGaps(step).map((message) => ({ step: step.name, message }))),
  ];
}

/** What the save intent consumes: the view plus the sidecar positions. */
function savePayload(view, positions) {
  return { view: serializableView(view), layout: positions };
}

function serializableView(view) {
  return {
    ...view,
    steps: view.steps.map((step) => {
      const cleaned = { ...step };
      delete cleaned.outgoing;
      return cleaned;
    }),
  };
}

function save(setSaveResult, onSaved, name, view, positions) {
  return fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "save-pipeline", pipeline: name, ...savePayload(view, positions) }),
  })
    .then((response) => (response.ok ? response.json() : null))
    .then((result) => {
      setSaveResult(result ?? { ok: false, findings: [] });
      if (result?.ok) {
        onSaved?.(result.state);
      }
      return result;
    });
}
