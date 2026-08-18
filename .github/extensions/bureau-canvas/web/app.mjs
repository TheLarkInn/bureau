import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
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

const h = React.createElement;
const CARD_WIDTH = 240;
const CARD_HEIGHT = 112;
const CONFIG_PAD = 72;
const FRAME_PAD = 34;
const flowItemTypes = {
  stepCard: StepCard,
  terminalPill: TerminalPill,
  concurrentFrame: ConcurrentFrame,
};
const flowEdgeTypes = { routed: RoutedEdge };
const edgeColors = {
  success: "var(--outcome-success)",
  failure: "var(--outcome-failure)",
  blocked: "var(--outcome-blocked)",
  "no-work": "var(--outcome-no-work)",
  data: "var(--relation-data)",
  observes: "var(--relation-observes)",
};

createRoot(document.querySelector("#root")).render(h(App));
window.__bureauCanvasMounted = true;
window.dispatchEvent(new Event("bureau-mounted"));

function App() {
  const [state, setState] = useState(null);
  const [selectedStep, setSelectedStep] = useState(null);

  useEffect(() => {
    let alive = true;
    fetch("./state", { cache: "no-store" })
      .then((response) => response.json())
      .then((next) => alive && setState(next));
    const events = new EventSource("./events");
    const localState = (event) => setState(event.detail);
    events.addEventListener("state", (event) => setState(JSON.parse(event.data)));
    events.addEventListener("focus", (event) => applyFocus(JSON.parse(event.data), setSelectedStep, setState));
    window.addEventListener("bureau-state", localState);
    return () => {
      alive = false;
      events.close();
      window.removeEventListener("bureau-state", localState);
    };
  }, []);

  if (!state) {
    return h("main", { className: "app-shell" }, h("p", { className: "status" }, "Loading…"));
  }

  return h(
    "main",
    { className: "app-shell" },
    h(Header, { state }),
    h(Findings, { className: "general-findings", findings: state.generalFindings ?? [] }),
    state.selectedPipeline
      ? h(PipelineView, { state, selectedStep, setSelectedStep })
      : h(ConfigView, { state }),
  );
}

function applyFocus(payload, setSelectedStep, setState) {
  const focus = payload?.focus;
  if (focus?.kind === "step") {
    setSelectedStep(focus.step ?? focus.name ?? null);
  }
  if (focus?.kind === "pipeline") {
    postIntent({ kind: "open-pipeline", pipeline: focus.name ?? focus.pipeline }).then((result) => {
      if (result?.ok) {
        setState(result.state);
      }
    });
  }
}

function Header({ state }) {
  const view = state.config?.view ?? emptyConfigView();
  return h(
    "header",
    { className: "app-header" },
    h("div", {}, h("h1", {}, "Bureau config"), h("p", { className: "summary" }, summaryText(view))),
    h(
      "div",
      { className: "status", "aria-live": "polite" },
      h("p", {}, state.status),
      h("p", {}, state.validation?.dir ?? state.dir),
      h("p", {}, `${view.orphans.length} orphan${view.orphans.length === 1 ? "" : "s"}`),
    ),
  );
}

function emptyConfigView() {
  return { assignments: [], roles: [], repos: [], pipelines: [], orphans: [] };
}

function summaryText(view) {
  return `${view.assignments.length} assignment · ${view.roles.length} roles · ${view.repos.length} repos · ${view.pipelines.length} pipelines`;
}

function ConfigView({ state }) {
  const layout = state.config?.layout ?? { items: [], edges: [] };
  const byId = new Map(layout.items.map((item) => [item.id, item]));
  const size = configSize(layout.items);
  return h(
    "section",
    { className: "view-shell" },
    h(
      "section",
      { className: "surface config-surface", style: { width: size.width, height: size.height }, "aria-label": "Bureau config view" },
      h("svg", { className: "edge-layer", viewBox: `0 0 ${size.width} ${size.height}`, width: size.width, height: size.height, "aria-hidden": true }, layout.edges.map((edge) => configEdge(edge, byId))),
      h("div", { className: "card-layer" }, layout.items.map((item) => h(ConfigCard, { key: item.id, state, item }))),
    ),
  );
}

function configSize(items) {
  return {
    width: Math.max(0, ...items.map((item) => item.x)) + CARD_WIDTH + CONFIG_PAD,
    height: Math.max(0, ...items.map((item) => item.y)) + CARD_HEIGHT + CONFIG_PAD,
  };
}

function configEdge(edge, byId) {
  const source = byId.get(edge.source);
  const target = byId.get(edge.target);
  if (!source || !target) {
    return null;
  }
  return h("line", {
    key: edge.id,
    className: "edge-path",
    x1: source.x + CARD_WIDTH,
    y1: source.y + CARD_HEIGHT / 2,
    x2: target.x,
    y2: target.y + CARD_HEIGHT / 2,
  });
}

function ConfigCard({ state, item }) {
  const data = configData(state, item);
  const className = `card card--${item.kind}${item.orphan ? " card--orphan" : ""}`;
  return h(
    "article",
    { className, style: { transform: `translate(${item.x}px, ${item.y}px)` }, "data-ref": item.id },
    item.kind === "pipeline"
      ? h("button", { className: "card-button", type: "button", onClick: () => selectPipeline(item.name) }, configCardContent(state, item, data))
      : h("div", {}, configCardContent(state, item, data)),
  );
}

function configCardContent(state, item, data) {
  return [
    h("p", { className: "kind-label", key: "kind" }, item.kind.replace("-", " ")),
    h("h2", { key: "title" }, item.name),
    h("p", { className: "detail", key: "detail", title: detailText(item, data) }, detailText(item, data)),
    h(Chips, { key: "chips", chips: chipsFor(state, item, data) }),
    item.kind === "pipeline" ? h(StepBadges, { key: "steps", state, name: item.name }) : null,
    h(Findings, { key: "findings", findings: state.findingsByItem?.[item.id] ?? [] }),
  ];
}

function configData(state, item) {
  const view = state.config?.view ?? emptyConfigView();
  const sources = { assignment: view.assignments, role: view.roles, repo: view.repos, pipeline: view.pipelines };
  if (item.kind === "work-source") {
    return view.assignments.find((assignment) => `work-source:${assignment.name}` === item.id) ?? {};
  }
  return (sources[item.kind] ?? []).find((value) => value.name === item.name) ?? {};
}

function detailText(item, data) {
  if (item.kind === "work-source") {
    return `${data.work?.forge ?? "unknown"} · ${data.work?.source ?? item.name}`;
  }
  if (item.kind === "assignment") {
    return `verify: ${data.verify ?? "not set"}`;
  }
  if (item.kind === "role") {
    return `${data.adapter ?? "adapter"} · min_trust: ${data.minTrust ?? "unknown"}`;
  }
  if (item.kind === "repo") {
    return data.access ?? "access unknown";
  }
  return `${data.stepCount ?? 0} steps`;
}

function chipsFor(state, item, data) {
  if (item.kind === "work-source") {
    return [data.work?.filter, data.work?.approvalLabel ? `approval: ${data.work.approvalLabel}` : null].filter(Boolean).map((text) => ({ text }));
  }
  if (item.kind === "assignment") {
    return Object.entries(data.limits ?? {}).filter(([, value]) => value != null).map(([name, value]) => ({ text: `${name}: ${value}` }));
  }
  if (item.kind === "role") {
    return (data.permissions ?? []).map((permission) => ({ text: permission, refs: data.usedBy ?? [], className: "permission-chip" }));
  }
  if (item.kind === "repo") {
    const primaries = (state.config?.view?.assignments ?? []).filter((assignment) => assignment.primaryRepo === data.name);
    return [{ text: data.access }, ...primaries.map((assignment) => ({ text: `primary: ${assignment.name}` }))];
  }
  const counts = state.pipelines?.[data.name]?.summary?.kindCounts ?? {};
  return Object.entries(counts).map(([kind, count]) => ({ text: `${kind}×${count}` }));
}

function Chips({ chips }) {
  if (!chips?.length) {
    return null;
  }
  return h("div", { className: "chips" }, chips.map((chip) => h(Chip, { key: `${chip.text}:${chip.refs?.join("|") ?? ""}`, chip })));
}

function Chip({ chip }) {
  return h("span", {
    className: `chip ${chip.className ?? ""}`.trim(),
    tabIndex: chip.refs ? 0 : undefined,
    onPointerEnter: chip.refs ? () => setHighlights(chip.refs) : undefined,
    onPointerLeave: chip.refs ? clearHighlights : undefined,
    onFocus: chip.refs ? () => setHighlights(chip.refs) : undefined,
    onBlur: chip.refs ? clearHighlights : undefined,
  }, chip.text);
}

function StepBadges({ state, name }) {
  const steps = state.pipelines?.[name]?.summary?.agentSteps ?? [];
  if (steps.length === 0) {
    return null;
  }
  return h("div", { className: "step-list" }, steps.map((step) => h("span", { key: step.ref, className: "step-badge", "data-ref": step.ref }, `${step.name} · ${step.role} · trust: ${step.trust ?? "role"}`)));
}

function PipelineView({ state, selectedStep, setSelectedStep }) {
  const name = state.selectedPipeline.name;
  const pipeline = state.pipelines?.[name];
  const flow = useMemo(() => toFlow(pipeline, state, selectedStep), [pipeline, state, selectedStep]);
  return h(
    "section",
    { className: "view-shell view-shell--pipeline" },
    h(
      "section",
      { className: "pipeline-main" },
      h("div", { className: "pipeline-toolbar" }, h("button", { className: "back-button", type: "button", onClick: backToConfig }, "Back to config"), h("h2", {}, name)),
      h(
        "div",
        { className: "pipeline-flow" },
        h(ReactFlow, {
          nodes: flow.nodes,
          edges: flow.edges,
          nodeTypes: flowItemTypes,
          edgeTypes: flowEdgeTypes,
          fitView: true,
          fitViewOptions: { padding: 0.22 },
          minZoom: 0.2,
          maxZoom: 1.5,
          nodesDraggable: false,
          nodesConnectable: false,
          elementsSelectable: true,
          proOptions: { hideAttribution: true },
          onNodeClick: (_, item) => item.type === "stepCard" && setSelectedStep(item.data.step.id),
        }, h(Background, { gap: 24, size: 1.5 }), h(Controls), h(MiniMap, { pannable: true, zoomable: true })),
      ),
    ),
    h(SidePanel, { state, pipeline, name }),
  );
}

function toFlow(pipeline, state, selectedStep) {
  const layout = pipeline?.layout ?? { steps: [], terminals: [], edges: [] };
  const handles = pipeline?.handles ?? { items: {}, edges: {} };
  const frames = (pipeline?.containers ?? []).map((frame) => flowFrame(frame));
  const steps = layout.steps.map((step) => flowStep(step, state, layout.name, handles.items[step.id], selectedStep));
  const terminals = layout.terminals.map((terminal) => flowTerminal(terminal, handles.items[terminal.id]));
  const backIndexes = routeIndexes(layout.edges, "back");
  return {
    nodes: [...frames, ...steps, ...terminals],
    edges: layout.edges.map((edge) => flowEdge(edge, handles.edges[edge.id], backIndexes.get(edge.id) ?? 0)),
  };
}

function flowFrame(frame) {
  return {
    id: frame.id,
    type: "concurrentFrame",
    position: { x: frame.x - FRAME_PAD, y: frame.y - FRAME_PAD },
    data: { frame },
    style: { width: frame.width + CARD_WIDTH + FRAME_PAD * 2, height: frame.height + CARD_HEIGHT + FRAME_PAD * 2 },
    selectable: false,
    draggable: false,
    zIndex: -1,
  };
}

function flowStep(step, state, pipelineName, handles, selectedStep) {
  const ref = `pipeline:${pipelineName}/${step.name}`;
  return {
    id: step.id,
    type: "stepCard",
    position: { x: step.x, y: step.y },
    data: { step, handles: handles ?? emptyHandles(), findings: state.findingsByStep?.[ref] ?? [], selected: selectedStep === step.name },
    style: { width: CARD_WIDTH },
    draggable: false,
  };
}

function flowTerminal(terminal, handles) {
  return {
    id: terminal.id,
    type: "terminalPill",
    position: { x: terminal.x, y: terminal.y + 26 },
    data: { terminal, handles: handles ?? emptyHandles() },
    style: { width: 136 },
    draggable: false,
  };
}

function flowEdge(edge, endpoints, backIndex) {
  const key = edge.relation === "control" ? edge.outcome : edge.relation;
  return {
    id: edge.id,
    source: edge.source,
    target: edge.target,
    sourceHandle: endpoints?.source,
    targetHandle: endpoints?.target,
    type: "routed",
    className: `flow-edge--${key}`,
    markerEnd: { type: MarkerType.ArrowClosed, color: edgeColors[key] ?? edgeColors.success },
    data: {
      label: edgeLabelText(edge),
      offset: edge.route === "back" ? 26 + backIndex * 12 : 12,
      captionShiftY: edgeCaptionShiftY(edge),
      route: edge.route,
    },
  };
}

function edgeLabelText(edge) {
  if (edge.relation === "control") {
    return edge.outcome;
  }
  return edge.relation === "observes" ? "over" : undefined;
}

function edgeCaptionShiftY(edge) {
  if (edge.relation !== "control") {
    return 0;
  }
  return { success: -18, failure: 0, blocked: 18, "no-work": 36 }[edge.outcome] ?? 0;
}

function routeIndexes(edges, route) {
  const counts = new Map();
  const indexes = new Map();
  for (const edge of edges.filter((edge) => edge.route === route)) {
    const key = edge.target;
    const index = counts.get(key) ?? 0;
    counts.set(key, index + 1);
    indexes.set(edge.id, index);
  }
  return indexes;
}

function RoutedEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, data = {} }) {
  const [path, labelX, labelY] = getSmoothStepPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    offset: data.offset ?? 12,
  });
  const [captionX, captionY] = edgeCaptionPosition({ data, labelX, labelY, sourceX, sourceY, targetX });
  return h(React.Fragment, null,
    h(BaseEdge, { id, path, markerEnd }),
    data.label ? h(EdgeLabelRenderer, null, h("div", {
      className: "react-flow__edge-label edge-caption",
      style: { transform: `translate(-50%, -50%) translate(${captionX}px, ${captionY}px)` },
    }, data.label)) : null,
  );
}

function edgeCaptionPosition({ data, labelX, labelY, sourceX, sourceY, targetX }) {
  if (data.route === "exit") {
    const direction = Math.sign(targetX - sourceX) || 1;
    const distance = Math.min(180, Math.max(96, Math.abs(targetX - sourceX) * 0.4));
    return [sourceX + direction * distance, sourceY - 10];
  }
  return [labelX, labelY + (data.captionShiftY ?? 0)];
}

function StepCard({ data }) {
  const step = data.step;
  const className = ["flow-card", `flow-card--${step.kind}`, step.parentId ? "flow-card--member" : "", data.selected ? "is-highlighted" : "", unreachableClass(data.findings)].filter(Boolean).join(" ");
  return h(
    "article",
    { className },
    h(Handles, { handles: data.handles }),
    h("button", { className: "step-button", type: "button" },
      h("p", { className: "kind-label" }, step.kind),
      h("h2", {}, step.name),
      h("p", { className: "detail", title: stepDetail(step) }, stepDetail(step)),
      h(Chips, { chips: stepChips(step) }),
      h(Findings, { findings: data.findings }),
    ),
  );
}

function TerminalPill({ data }) {
  return h("article", { className: "flow-card terminal-pill" }, h(Handles, { handles: data.handles }), h("h2", {}, data.terminal.name));
}

function ConcurrentFrame() {
  return h("div", { className: "concurrent-frame" });
}

function Handles({ handles }) {
  return [
    ...(handles.target ?? []).map((handle, index, list) => handleElement(handle, "target", index, list)),
    ...(handles.source ?? []).map((handle, index, list) => handleElement(handle, "source", index, list)),
  ];
}

function handleElement(handle, type, index, list) {
  return h(Handle, {
    key: `${type}:${handle.id}`,
    id: handle.id,
    type,
    position: handlePosition(handle.side),
    isConnectable: false,
    className: `flow-handle flow-handle--${handle.name}`,
    style: handleStyle(handle.side, index, list),
  });
}

function handlePosition(side) {
  return { top: Position.Top, right: Position.Right, bottom: Position.Bottom, left: Position.Left }[side];
}

function handleStyle(side, index, list) {
  const percent = list.length <= 1 ? 50 : 22 + (index * 56) / (list.length - 1);
  if (side === "left" || side === "right") {
    return { top: `${percent}%` };
  }
  return { left: `${percent}%` };
}

function emptyHandles() {
  return { source: [], target: [] };
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
  return `${step.fields.members?.length ?? 0} in parallel`;
}

function stepChips(step) {
  return [
    step.parentId ? { text: `member of ${step.parentId}` } : null,
    step.fields.trust ? { text: `trust: ${step.fields.trust}` } : null,
    step.fields.maxAttempts > 1 ? { text: `attempts: ${step.fields.maxAttempts}` } : null,
  ].filter(Boolean);
}

function unreachableClass(findings) {
  return findings.some((finding) => /unreachable/i.test(finding.message ?? "")) ? "flow-card--unreachable" : "";
}

function SidePanel({ state, pipeline, name }) {
  const findings = pipelineFindings(state, name);
  return h(
    "aside",
    { className: "side-panel" },
    h("section", { className: "panel-section" }, h("h2", {}, name), h("p", { className: "muted" }, pipelineCounts(pipeline))),
    h("section", { className: "panel-section" }, h("h3", {}, `Validation (${findings.length})`), findings.length ? h(Findings, { findings }) : h("p", { className: "muted" }, "clean — bureau validate would pass")),
    h("section", { className: "panel-section" }, h("h3", {}, "Legend"), h(Legend)),
    h("section", { className: "panel-section" }, h("h3", {}, "Trust flow"), h("p", { className: "muted" }, "Reserved for trust analysis.")),
  );
}

function pipelineCounts(pipeline) {
  const steps = pipeline?.layout?.steps?.length ?? 0;
  const terminals = pipeline?.layout?.terminals?.length ?? 0;
  const edges = pipeline?.layout?.edges?.length ?? 0;
  return `${steps} steps · ${terminals} terminals · ${edges} edges`;
}

function pipelineFindings(state, name) {
  return (state.findings ?? []).filter((finding) => {
    const target = finding.target ?? {};
    return target.pipeline === name || target.kind === "pipeline" && target.pipeline === name;
  });
}

function Legend() {
  return h("div", { className: "legend" }, [
    legendItem("success", "var(--outcome-success)"),
    legendItem("failure", "var(--outcome-failure)"),
    legendItem("blocked", "var(--outcome-blocked)"),
    legendItem("no-work", "var(--outcome-no-work)"),
    legendItem("inputs_from", "var(--relation-data)", "legend-swatch--data"),
    legendItem("over", "var(--relation-observes)", "legend-swatch--observes"),
  ]);
}

function legendItem(text, color, className = "") {
  return h("span", { className: "legend-item", key: text }, h("span", { className: `legend-swatch ${className}`.trim(), style: { "--swatch": color } }), ` ${text}`);
}

function Findings({ findings, className = "findings" }) {
  if (!findings?.length) {
    return null;
  }
  return h("div", { className }, findings.map((finding, index) => h("span", { className: findingClass(finding), key: `${finding.message}:${index}` }, finding.message)));
}

function findingClass(finding) {
  const advisory = finding.marker === "advisory" || finding.source === "advisory";
  return `finding ${advisory ? "finding--advisory" : "finding--validation"}`;
}

function setHighlights(refs) {
  document.body.classList.add("has-highlight");
  for (const element of document.querySelectorAll("[data-ref]")) {
    element.classList.toggle("is-highlighted", refs.includes(element.dataset.ref));
  }
}

function clearHighlights() {
  document.body.classList.remove("has-highlight");
  for (const element of document.querySelectorAll(".is-highlighted")) {
    element.classList.remove("is-highlighted");
  }
}

function selectPipeline(name) {
  postIntent({ kind: "open-pipeline", pipeline: name }).then(publishLocalState);
}

function backToConfig() {
  postIntent({ kind: "back-to-config" }).then(publishLocalState);
}

function publishLocalState(result) {
  if (result?.ok) {
    window.dispatchEvent(new CustomEvent("bureau-state", { detail: result.state }));
  }
}

function postIntent(body) {
  return fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  }).then((response) => response.ok ? response.json() : null);
}