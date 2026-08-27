// The pipeline editor: full node/edge CRUD on one pipeline's step graph.
//
// The editor keeps a draft pipeline view in React state, applies edits
// through the pure transforms in `../editor/…` (`lib/edit.mjs` semantics
// mirrored client-side against the same view shape), and saves through the
// server's `save-pipeline` intent, which owns validation and the revert.
// Node positions ride along as the layout sidecar (Q10).

import React, { useEffect, useMemo, useRef, useState } from "react";
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

import { drawableEdges } from "../graph-edges.mjs";
import { MeasurementGuard } from "../graph-measure.mjs";
import { withoutReferencesTo } from "../step-refs.mjs";
import { layoutPipeline } from "../layout.js";
import { terminalCopy, terminalOption } from "../terminals.js";

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

export function PipelineEditor({ state, name, onSaved, onDirtyChange }) {
  const pipeline = state.pipelines?.[name];
  const [draft, setDraft] = useState(null);
  const [positions, setPositions] = useState(() => ({ ...(pipeline?.arrangement ?? {}) }));
  const [layoutDirty, setLayoutDirty] = useState(false);
  const [selected, setSelected] = useState(null);
  const [saveResult, setSaveResult] = useState(null);
  const [saving, setSaving] = useState(false);
  const [flowApi, setFlowApi] = useState(null);

  const view = useMemo(() => draft ?? editableView(pipeline?.view), [draft, pipeline]);
  const hints = useMemo(() => problems(view), [view]);
  const flow = useMemo(() => toFlow(view, positions, hints, saveResult), [view, positions, hints, saveResult]);

  const edit = (next) => {
    setDraft(next);
    setSaveResult(null);
  };
  const selectedStep = view.steps.find((step) => step.name === selected) ?? null;
  const dirty = draft != null || layoutDirty;
  const invalidNumbers = view.steps.some((step) =>
    !positiveInteger(step.fields.maxAttempts)
    || (step.kind === "concurrent" && !positiveInteger(step.fields.maxConcurrent ?? 1)));
  useEffect(() => {
    onDirtyChange?.(dirty);
    const beforeUnload = (event) => {
      if (dirty) {
        event.preventDefault();
        event.returnValue = "";
      }
    };
    const escape = (event) => event.key === "Escape" && setSelected(null);
    window.addEventListener("beforeunload", beforeUnload);
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("beforeunload", beforeUnload);
      window.removeEventListener("keydown", escape);
    };
  }, [dirty, onDirtyChange]);
  useEffect(() => {
    const refit = () => flowApi?.fitView({ padding: 0.22, duration: 150 });
    window.addEventListener("resize", refit);
    return () => window.removeEventListener("resize", refit);
  }, [flowApi]);
  // A step added at a new layer can land outside the visible canvas, which
  // reads as "nothing happened". Refit when the graph *gains* a node — not on
  // mount, where React Flow's own `fitView` already ran and a second animated
  // one would only keep the nodes moving.
  const stepCount = view.steps.length;
  const lastCount = useRef(stepCount);
  useEffect(() => {
    if (stepCount > lastCount.current) {
      flowApi?.fitView({ padding: 0.22, duration: 200 });
    }
    lastCount.current = stepCount;
  }, [flowApi, stepCount]);
  const focusStep = (step) => {
    setSelected(step);
    const node = flow.nodes.find((candidate) => candidate.id === step);
    if (node && flowApi) {
      flowApi.setCenter(node.position.x + 120, node.position.y + 60, { zoom: 1, duration: 250 });
    }
  };
  const discard = () => {
    setDraft(null);
    setPositions({ ...(pipeline?.arrangement ?? {}) });
    setLayoutDirty(false);
    setSaveResult(null);
    setSelected(null);
  };
  /*
   * A save in flight has to withhold its own button. `save-pipeline` writes the
   * pipeline file, re-validates and reverts, so a second click while the first
   * is outstanding is a second write racing the revert of the first. The button
   * used to stay live for the whole round trip, and nothing caught it because
   * the in-flight screen was an excluded state — the registry asserted the
   * button was withheld and the matrix never rendered it to find out.
   */
  const saveCurrent = () => {
    setSaving(true);
    return save({ setSaveResult, setDraft, setLayoutDirty, onSaved, name, view, positions })
      .finally(() => setSaving(false));
  };

  return h(
    "section",
    { className: "editor-shell" },
    h(EditorToolbar, {
      name,
      view,
      dirty,
      hints,
      saveResult,
      invalidNumbers,
      saving,
      onSave: saveCurrent,
      onDiscard: discard,
      onAdd: (kind) => addStep(name, view, kind, edit, setSelected),
    }),
    h(
      "div",
      { className: "editor-main" },
      h(
        "div",
        { className: "editor-flow", "data-graph-edges": String(flow.declared) },
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
            onInit: setFlowApi,
            deleteKeyCode: null,
            proOptions: { hideAttribution: true },
            onNodeClick: (_, node) => {
              if (node.type === "stepNode") {
                setSelected(node.id);
              }
            },
            onNodeKeyDown: (event, node) => {
              if (node.type === "stepNode" && (event.key === "Enter" || event.key === " ")) {
                event.preventDefault();
                setSelected(node.id);
              }
            },
            onNodeDragStop: (_, node) => {
              setPositions((current) => ({ ...current, [node.id]: { x: Math.round(node.position.x), y: Math.round(node.position.y) } }));
              setLayoutDirty(true);
              setSaveResult(null);
            },
            onConnect: (connection) => connect(view, connection, edit),
          },
          h(Background, { gap: 24, size: 1.5 }),
          h(Controls),
          h(MiniMap, { pannable: true, zoomable: true, "aria-label": "Pipeline overview", nodeColor: minimapColor }),
          h(MeasurementGuard, { ids: flow.nodes.map((node) => node.id) }),
        ),
      ),
      h(SidePanel, {
        view,
        step: selectedStep,
        roles: state.config?.view?.roles ?? [],
        hints,
        saveResult,
        onChange: (next) => edit(next),
        onClose: () => setSelected(null),
        onIssue: focusStep,
        onRename: selectedStep ? (to) => {
          const next = renameStep(view, selectedStep.name, to);
          if (next !== view) {
            setPositions((current) => {
              const position = current[selectedStep.name];
              if (!position) {
                return current;
              }
              const moved = { ...current, [to]: position };
              delete moved[selectedStep.name];
              return moved;
            });
            edit(next);
            setSelected(to);
          }
        } : null,
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

/**
 * The step gone, its edges gone, and every reference to it in another step's
 * fields gone with them.
 *
 * The field rule is `web/step-refs.mjs`, shared with `lib/edit.mjs`, because
 * dangling on delete is one defect however it is reached: a decision routes
 * through its `on:` map and an agent step through `inputs_from`, neither of
 * which is in `view.edges`, so dropping edges alone left the pipeline naming a
 * step that no longer exists. React Flow draws nothing for an edge whose
 * endpoint is missing, so the graph looked clean either way — this is the case
 * the screen cannot show, which is why it has to be true by construction.
 */
function removeStep(view, stepName) {
  return {
    ...view,
    steps: view.steps.filter((step) => step.name !== stepName).map((step) => withoutReferencesTo(step, stepName)),
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
  const edges = effectiveEdges(view);
  const terminals = referencedTerminals(edges);
  const auto = layoutPipeline({ ...view, edges, terminals });
  const marked = new Map();
  for (const hint of hints) {
    marked.set(hint.step, [...(marked.get(hint.step) ?? []), hint.message]);
  }

  function referencedTerminals(edges) {
    const used = new Set(edges.filter((edge) => edge.target.startsWith("terminal:")).map((edge) => edge.target));
    return TERMINALS
      .map((name) => ({ id: `terminal:${name}`, name, type: "terminal" }))
      .filter((terminal) => used.has(terminal.id));
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
    // Counted from the view and its referenced terminals, never from the two
    // arrays above: see `web/graph-edges.mjs`.
    declared: drawableEdges([...view.steps.map((step) => ({ id: step.name })), ...terminals], edges),
  };
}

function effectiveEdges(view) {
  const ordinary = view.edges.filter((edge) => {
    const source = view.steps.find((step) => step.name === edge.source);
    return edge.relation === "control" && source?.kind !== "decision";
  });
  const decisions = view.steps
    .filter((step) => step.kind === "decision")
    .flatMap((step) => Object.entries(step.fields.on ?? {})
      .filter(([outcome, target]) => OUTCOMES.includes(outcome) && present(target))
      .map(([outcome, target]) => ({
        id: `control:${step.name}:${outcome}->${TERMINALS.includes(target) ? `terminal:${target}` : target}`,
        source: step.name,
        target: TERMINALS.includes(target) ? `terminal:${target}` : target,
        relation: "control",
        outcome,
      })));
  const dependencies = view.steps.flatMap((step) => [
    ...(step.fields.inputsFrom ?? []).filter(present).map((source) => ({
      id: `data:${source}->${step.name}`,
      source,
      target: step.name,
      relation: "data",
    })),
    ...(step.kind === "decision" && present(step.fields.over) ? [{
      id: `observes:${step.fields.over}->${step.name}`,
      source: step.fields.over,
      target: step.name,
      relation: "observes",
    }] : []),
  ]);
  return [...ordinary, ...decisions, ...dependencies];
}

function minimapColor(node) {
  if (node.type === "terminalNode") {
    return "var(--text-color-muted, #656d76)";
  }
  return `var(--kind-${node.data?.step?.kind}, #656d76)`;
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
  const edges = effectiveEdges(view).map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    sourceHandle: `out:${edge.outcome ?? edge.relation}`,
    targetHandle: "in",
    type: "outcome",
    focusable: false,
    selectable: false,
    label: edge.relation === "control" ? edge.outcome : edge.relation,
    className: `editor-edge--${edge.relation === "control" ? edge.outcome : edge.relation}`,
    markerEnd: { type: MarkerType.ArrowClosed },
    data: edge.relation === "control" ? { editable: true, sourceStep: edge.source, outcome: edge.outcome } : { editable: false },
  }));
  return spreadParallelLabels(edges);
}

function spreadParallelLabels(edges) {
  const groups = new Map();
  const terminalTargets = [...new Set(edges.filter((edge) => edge.target.startsWith("terminal:")).map((edge) => edge.target))].sort();
  for (const edge of edges) {
    const key = edge.target.startsWith("terminal:") ? edge.target : `${edge.source}->${edge.target}`;
    groups.set(key, [...(groups.get(key) ?? []), edge]);
  }
  return edges.map((edge) => {
    const terminalLabel = edge.target.startsWith("terminal:");
    const key = terminalLabel ? edge.target : `${edge.source}->${edge.target}`;
    const siblings = groups.get(key);
    const index = siblings.indexOf(edge);
    const labelOffset = (index - (siblings.length - 1) / 2) * 28;
    return {
      ...edge,
      data: { ...edge.data, labelOffset, terminalLabel, terminalColumn: terminalTargets.indexOf(edge.target) },
    };
  });
}

// --- nodes ---

function StepNode({ data, selected }) {
  const step = data.step;
  const detail = stepDetail(step);
  const classes = ["editor-card", `editor-card--${step.kind}`, data.hints.length ? "editor-card--hint" : "", selected ? "is-highlighted" : ""].filter(Boolean).join(" ");
  return h(
    "article",
    { className: classes, "data-ref": step.name },
    h(Handle, { id: "in", type: "target", position: Position.Left, className: "editor-handle" }),
    OUTCOMES.map((outcome) =>
      h(Handle, {
        key: outcome,
        id: `out:${outcome}`,
        type: "source",
        position: Position.Right,
        className: `editor-handle editor-handle--${outcome}`,
        style: { top: `${14 + OUTCOMES.indexOf(outcome) * 24}%` },
        title: outcome,
      }),
    ),
    h(Handle, {
      id: "out:data",
      type: "source",
      position: Position.Bottom,
      className: "editor-handle editor-handle--data",
      style: { left: "38%" },
      title: "inputs_from",
    }),
    h(Handle, {
      id: "out:observes",
      type: "source",
      position: Position.Bottom,
      className: "editor-handle editor-handle--observes",
      style: { left: "62%" },
      title: "observes",
    }),
    h("p", { className: "kind-label" }, step.kind),
    h("h2", {}, step.name),
    h("p", { className: "detail", title: detail }, detail),
    data.hints.length
      ? h("span", { className: "editor-card__issue-count" }, `${data.hints.length} issue${data.hints.length === 1 ? "" : "s"}`)
      : null,
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
  const copy = terminalCopy(data.name);
  return h(
    "article",
    { className: `editor-terminal editor-terminal--${data.name}` },
    h(Handle, { id: "in", type: "target", position: Position.Left, className: "editor-handle" }),
    h("h2", {}, copy.label),
    h("p", {}, copy.detail),
  );
}

function OutcomeEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, label, data }) {
  const [path, labelX, labelY] = getSmoothStepPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition, offset: 16 });
  const captionX = data?.terminalLabel ? targetX - 72 - data.terminalColumn * 56 : labelX;
  const captionY = (data?.terminalLabel ? targetY : labelY) + (data?.labelOffset ?? 0);
  return h(
    React.Fragment,
    null,
    h(BaseEdge, { id, path, markerEnd }),
    label
      ? h(EdgeLabelRenderer, null, h("div", {
          className: "react-flow__edge-label edge-caption",
          style: { transform: `translate(-50%, -50%) translate(${captionX}px, ${captionY}px)` },
        }, label))
      : null,
  );
}

// --- toolbar + side panel ---

function EditorToolbar({ dirty, hints, saveResult, invalidNumbers, saving, onSave, onDiscard, onAdd }) {
  const [kind, setKind] = useState("deterministic");
  return h(
    "div",
    { className: "editor-toolbar" },
    h("h2", {}, "Steps"),
    h(
      "span",
      { className: "editor-toolbar-add" },
      h(
        "select",
        {
          className: "form-control form-select",
          value: kind,
          onChange: (event) => setKind(event.target.value),
          "aria-label": "New step kind",
        },
        STEP_KINDS.map((option) => h("option", { key: option, value: option }, option)),
      ),
      h("button", { type: "button", className: "btn", "data-testid": "editor-add-step", onClick: () => onAdd(kind) }, "+ Add step"),
    ),
    h("span", { className: `editor-status${statusTone(hints, saveResult)}` }, statusText(dirty, hints, saveResult)),
    h("span", { className: "editor-legend", "aria-label": "Edge legend" },
      h("span", { className: "legend-swatch legend-swatch--success" }, "success"),
      h("span", { className: "legend-swatch legend-swatch--failure" }, "failure"),
      h("span", { className: "legend-swatch legend-swatch--blocked" }, "blocked"),
      h("span", { className: "legend-swatch legend-swatch--data" }, "data")),
    // Discard and Save are one control group, not two toolbar cells. As
    // separate grid items the compact toolbar wrapped between them and left
    // Save alone on a row, aligned to the end of the *first* column — a primary
    // action floating in the middle of the bar, away from the choice it belongs
    // to.
    h(
      "span",
      { className: "editor-toolbar-actions" },
      dirty ? h("button", { type: "button", className: "btn", "data-testid": "editor-discard", disabled: saving, onClick: onDiscard }, "Discard changes") : null,
      h("button", { type: "button", className: "btn btn--primary", "data-testid": "editor-save", disabled: !dirty || invalidNumbers || saving, onClick: onSave }, saving ? "Saving…" : "Save changes"),
    ),
  );
}

/*
 * "see findings" was a redirect to a place the reader could not see. The list
 * it pointed at is drawn at the bottom of a side panel that scrolls its own
 * content, below a step form long enough to push it off-screen on both
 * viewports — and when the transport died there were no findings at all, only
 * an error string, so the redirect was also untrue. The panel now opens with
 * the refusal, so the toolbar reports the event and stops directing traffic.
 */
function statusText(dirty, hints, saveResult) {
  if (saveResult && !saveResult.ok) {
    return "save reverted";
  }
  if (hints.length) {
    return `${hints.length} issue${hints.length === 1 ? "" : "s"}`;
  }
  return dirty ? "unsaved edits" : "saved";
}

/** A refusal outranks a hint: one already happened, the other is advice. */
function statusTone(hints, saveResult) {
  if (saveResult && !saveResult.ok) {
    return " editor-status--error";
  }
  return hints.length ? " editor-status--hints" : "";
}

/*
 * The refusal is drawn first, above the step form, because it is the answer to
 * the click the reader just made. Appended last it landed below a form tall
 * enough to push it out of a panel that scrolls its own content — so on the one
 * state where the reader needs a reason, the reason was the part they could not
 * see, and the toolbar sent them to it by name.
 */
function SidePanel({ view, step, roles, hints, saveResult, onChange, onClose, onDelete, onIssue, onRename }) {
  return h(
    "aside",
    { className: "editor-panel" },
    saveResult && !saveResult.ok
      ? h("section", { className: "panel-section editor-save-reverted", "data-testid": "editor-save-reverted" }, h("h3", {}, "Save reverted"), h("ul", { className: "editor-issues" },
          (saveResult.findings?.length ? saveResult.findings : [{ message: saveResult.error ?? "save failed" }])
            .map((finding, index) => h("li", { key: index }, finding.message))))
      : null,
    step
      ? h(StepEditor, { view, step, roles, onChange, onClose, onDelete, onRename })
      : h(
          "div",
          { className: "editor-empty" },
          h("h3", {}, "Edit a step"),
          h("p", { className: "muted" }, "Select a step to edit its fields and outcomes."),
          h("p", { className: "muted" }, "Drag from a right-edge outcome handle to another node to rewire."),
        ),
    hints.length
      ? h("section", { className: "panel-section" }, h("h3", {}, `Issues (${hints.length})`), h("ul", { className: "editor-issues" }, hints.map((hint) =>
          h("li", { key: `${hint.step}:${hint.message}` },
            h("button", { type: "button", onClick: () => onIssue(hint.step) }, `${hint.step}: ${hint.message}`)))))
      : null,
  );
}

function StepEditor({ view, step, roles, onChange, onClose, onDelete, onRename }) {
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [name, setName] = useState(step.name);
  useEffect(() => setName(step.name), [step.name]);
  const set = (field, value) => onChange(setField(view, step.name, field, value));
  const nameProblem = !name.trim()
    ? "A step name cannot be empty."
    : view.steps.some((candidate) => candidate.name !== step.name && candidate.name === name.trim())
      ? `A step named \`${name.trim()}\` already exists.`
      : null;
  const commitName = () => {
    if (!nameProblem && name.trim() !== step.name) {
      onRename(name.trim());
    }
  };
  return h(
    "section",
    { className: "panel-section editor-step" },
    h("div", { className: "editor-step-head" },
      h("div", {}, h("h3", {}, step.name), h("span", { className: `kind-label kind-label--${step.kind}` }, step.kind)),
      h("button", { type: "button", className: "btn btn--small", onClick: onClose }, "Close")),
    h("label", {}, "name", h("input", {
      className: `form-control form-control--mono${nameProblem ? " form-control--invalid" : ""}`,
      "data-testid": "editor-step-name",
      value: name,
      onChange: (event) => setName(event.target.value),
      onBlur: commitName,
      onKeyDown: (event) => {
        if (event.key === "Enter") {
          commitName();
        }
      },
    })),
    nameProblem ? h("p", { className: "editor-hints" }, nameProblem) : null,
    h(KindFields, { view, step, roles, set }),
    h(DependencyFields, { view, step, set }),
    h(EdgeEditor, { view, step, onChange }),
    h("label", {}, "max attempts", h("input", {
      type: "number",
      min: 1,
      className: `form-control form-control--mono${positiveInteger(step.fields.maxAttempts) ? "" : " form-control--invalid"}`,
      "data-testid": "editor-max-attempts",
      value: step.fields.maxAttempts ?? "",
      onChange: (event) => set("maxAttempts", numberInput(event.target.value)),
    })),
    positiveInteger(step.fields.maxAttempts) ? null : h("p", { className: "editor-hints" }, "Max attempts must be a whole number of at least 1."),
    confirmDelete
      ? h(
          "div",
          { className: "editor-danger-zone" },
          h("p", {}, `Delete \`${step.name}\` and every edge connected to it?`),
          h("div", { className: "editor-danger-actions" },
            h("button", { type: "button", className: "btn btn--danger", "data-testid": "editor-delete-confirm", onClick: onDelete }, "Delete step"),
            h("button", { type: "button", className: "btn", onClick: () => setConfirmDelete(false) }, "Keep step")),
        )
      : h("button", {
          type: "button",
          className: "btn btn--danger",
          "data-testid": "editor-delete-step",
          onClick: () => setConfirmDelete(true),
        }, "Delete step"),
  );
}

function setField(view, name, field, value) {
  return {
    ...view,
    steps: view.steps.map((step) => (step.name === name ? { ...step, fields: { ...step.fields, [field]: value } } : step)),
  };
}

function positiveInteger(value) {
  return Number.isInteger(value) && value >= 1;
}

function numberInput(value) {
  return value === "" ? null : Number(value);
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
  })).map((edge) => ({ ...edge, id: edgeIdentifier(edge) }));
  return syncSteps({ ...view, steps, edges });
}

function edgeIdentifier(edge) {
  const outcome = edge.outcome ? `:${edge.outcome}` : "";
  return `${edge.relation}:${edge.source}${outcome}->${edge.target}`;
}

function KindFields({ view, step, roles, set }) {
  if (step.kind === "deterministic") {
    return h("label", {}, "run", h("textarea", {
      className: "form-control form-control--mono editor-textarea",
      value: step.fields.run ?? "",
      onChange: (event) => set("run", event.target.value),
    }));
  }
  if (step.kind === "agent") {
    return h(React.Fragment, null,
      h("label", {}, "role", h("select", {
        className: "form-control form-select",
        "aria-label": "Step role",
        value: step.fields.role ?? "",
        onChange: (event) => set("role", event.target.value),
      },
      h("option", { value: "" }, "Choose a role"),
      roles.map((role) => h("option", { key: role.name, value: role.name }, role.name)))),
      h("label", {}, "minimum trust", h("select", {
        className: "form-control form-select",
        "aria-label": "Minimum trust",
        value: step.fields.trust ?? "",
        onChange: (event) => set("trust", event.target.value || null),
      },
      h("option", { value: "" }, "Inherit from role"),
      ["untrusted", "derived", "maintainer", "trusted"].map((trust) =>
        h("option", { key: trust, value: trust }, trust)))),
    );
  }
  if (step.kind === "decision") {
    return h("label", {}, "observe step", h("select", {
      className: "form-control form-select",
      value: step.fields.over ?? "",
      onChange: (event) => set("over", event.target.value),
    },
    h("option", { value: "" }, "Choose a step"),
    view.steps.filter((candidate) => candidate.name !== step.name).map((candidate) =>
      h("option", { key: candidate.name, value: candidate.name }, candidate.name))));
  }
  return h(React.Fragment, null,
    h("label", {}, "members (comma-separated)", h("input", {
      className: "form-control form-control--mono",
      value: (step.fields.members ?? []).join(", "),
      onChange: (event) => set("members", event.target.value.split(",").map((member) => member.trim()).filter(Boolean)),
    })),
    h("label", {}, "completion", h("select", {
      className: "form-control form-select",
      value: step.fields.completion ?? "all",
      onChange: (event) => set("completion", event.target.value),
    },
    h("option", { value: "all" }, "Wait for all members"),
    h("option", { value: "stop_on_failure" }, "Stop on first failure"))),
    h("label", {}, "maximum concurrent members", h("input", {
      type: "number",
      min: 1,
      className: `form-control form-control--mono${positiveInteger(step.fields.maxConcurrent ?? 1) ? "" : " form-control--invalid"}`,
      value: step.fields.maxConcurrent ?? "",
      placeholder: "unlimited",
      onChange: (event) => set("maxConcurrent", numberInput(event.target.value)),
    })),
  );
}

function DependencyFields({ view, step, set }) {
  const candidates = view.steps.filter((candidate) => candidate.name !== step.name);
  return h(
    "fieldset",
    { className: "editor-edges" },
    h("legend", {}, "data inputs"),
    candidates.map((candidate) => {
      const checked = (step.fields.inputsFrom ?? []).includes(candidate.name);
      return h("label", { className: "editor-check", key: candidate.name },
        h("input", {
          type: "checkbox",
          checked,
          onChange: () => set("inputsFrom", checked
            ? step.fields.inputsFrom.filter((name) => name !== candidate.name)
            : [...(step.fields.inputsFrom ?? []), candidate.name]),
        }),
        candidate.name);
    }),
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
  const valid = current == null || validTarget(view, current);
  return h(
    "label",
    {},
    outcome,
    h(
      "select",
      {
        className: `form-control form-select${valid ? "" : " form-control--invalid"}`,
        "aria-invalid": valid ? undefined : "true",
        value: current ?? "",
        onChange: (event) => onChange(setEdge(view, step.name, outcome, event.target.value || null)),
      },
      h("option", { value: "" }, "abort (default)"),
      valid ? null : h("option", { value: current }, `Unknown target: ${current}`),
      view.steps.filter((candidate) => candidate.name !== step.name).map((candidate) => h("option", { key: candidate.name, value: candidate.name }, candidate.name)),
      TERMINALS.map((terminal) => h("option", { key: terminal, value: terminal }, terminalOption(terminal))),
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
      h(DecisionOutcomeSelect, { key: outcome, view, step, outcome, on, onChange })),
    gaps.length ? h("p", { className: "editor-hints" }, gaps.join("; ")) : null,
  );
}

function DecisionOutcomeSelect({ view, step, outcome, on, onChange }) {
  const target = on[outcome] ?? "";
  const valid = !target || validTarget(view, target);
  return h(
    "label",
    {},
    outcome,
    h(
      "select",
      {
        className: `form-control form-select${valid ? "" : " form-control--invalid"}`,
        "aria-invalid": valid ? undefined : "true",
        value: target,
        onChange: (event) => onChange(setField(view, step.name, "on", { ...on, [outcome]: event.target.value })),
      },
      h("option", { value: "" }, "—"),
      valid ? null : h("option", { value: target }, `Unknown target: ${target}`),
      view.steps.filter((candidate) => candidate.name !== step.name).map((candidate) =>
        h("option", { key: candidate.name, value: candidate.name }, candidate.name)),
      TERMINALS.map((terminal) =>
        h("option", { key: terminal, value: terminal }, terminalOption(terminal))),
    ),
  );
}

function edgeTarget(view, source, outcome) {
  const edge = view.edges.find((candidate) => candidate.relation === "control" && candidate.source === source && candidate.outcome === outcome);
  return edge ? plainTarget(edge.target) : null;
}

function validTarget(view, target) {
  return TERMINALS.includes(target) || view.steps.some((step) => step.name === target);
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
  const edges = effectiveEdges(view);
  const reached = new Set(edges.filter((edge) => edge.relation === "control").map((edge) => edge.target));
  const known = new Set([...view.steps.map((step) => step.name), ...TERMINALS.map((name) => `terminal:${name}`)]);
  return [
    ...view.steps.filter((step, index) => index > 0 && !reached.has(step.name)).map((step) => ({ step: step.name, message: "step is not reachable by any control edge" })),
    ...edges
      .filter((edge) => edge.relation === "control" && !known.has(edge.target))
      .map((edge) => ({ step: edge.source, message: `\`${edge.outcome}\` edge points at \`${plainTarget(edge.target)}\`, which does not exist` })),
    // Ordinary steps fail closed: omitted outcomes route to abort by design.
    // Only decisions promise an explicit four-way outcome map.
    ...view.steps
      .filter((step) => step.kind === "decision")
      .flatMap((step) => decisionGaps(step).map((message) => ({ step: step.name, message }))),
    ...view.steps
      .filter((step) => !positiveInteger(step.fields.maxAttempts))
      .map((step) => ({ step: step.name, message: "max attempts must be a whole number of at least 1" })),
    ...view.steps
      .filter((step) => step.kind === "concurrent" && !positiveInteger(step.fields.maxConcurrent ?? 1))
      .map((step) => ({ step: step.name, message: "maximum concurrent members must be a whole number of at least 1" })),
    ...view.steps
      .filter((step) => step.kind === "concurrent")
      .flatMap((step) => (step.fields.members ?? [])
        .filter((member) => member === step.name || !view.steps.some((candidate) => candidate.name === member))
        .map((member) => ({ step: step.name, message: `member \`${member}\` is not another existing step` }))),
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

/*
 * The panel already draws `saveResult.error`; this is the branch that fills it.
 * A rejected `fetch` used to skip every `.then` here, so `saveResult` stayed as
 * it was and the toolbar fell back to "unsaved edits" — the same words it showed
 * before the click. A write that failed and a write never dispatched read
 * identically, which is the one thing a draft surface may not do.
 *
 * What it says is this surface's own sentence rather than the rejection's
 * message. `String(error.message)` put "Failed to fetch" in the panel: the
 * running browser's private wording for a dead socket, which Firefox words
 * differently, which names no pipeline, which promises nothing about the draft
 * it just failed to write, and which no state could assert for that reason.
 * Every sibling save on the config surface already answers a dead transport
 * with a product sentence, because `postIntent` swallows the rejection and each
 * caller supplies its own words. This was the one write that reached for the
 * transport's words instead.
 */
function save({ setSaveResult, setDraft, setLayoutDirty, onSaved, name, view, positions }) {
  return fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "save-pipeline", pipeline: name, ...savePayload(view, positions) }),
  })
    .then((response) => (response.ok ? response.json() : null))
    .then((result) => {
      setSaveResult(result ?? { ok: false, findings: [] });
      if (result?.ok) {
        setDraft(null);
        setLayoutDirty(false);
        onSaved?.(result.state);
      }
      return result;
    })
    .catch(() => {
      setSaveResult({ ok: false, findings: [], error: "could not save this pipeline — nothing was written" });
      return null;
    });
}
