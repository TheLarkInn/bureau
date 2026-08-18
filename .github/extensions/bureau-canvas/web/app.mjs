const CARD_WIDTH = 216;
const CARD_HEIGHT = 112;
const CARD_PAD = 64;
const MEMBER_PAD = 24;

const summary = document.querySelector("#summary");
const status = document.querySelector("#status");
const surface = document.querySelector("#config-view");
const edgeLayer = document.querySelector("#edges");
const cardLayer = document.querySelector("#cards");
const drillDown = document.querySelector("#drill-down");
const generalFindings = document.querySelector("#general-findings");

let currentState = null;
let selectedStep = null;

await loadState();

const events = new EventSource("./events");
events.addEventListener("state", (event) => receiveState(JSON.parse(event.data)));
events.addEventListener("focus", (event) => receiveFocus(JSON.parse(event.data)));

async function loadState() {
  const response = await fetch("./state", { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`state request failed: ${response.status}`);
  }
  render(await response.json());
}

function receiveState(payload) {
  if (payload?.config) {
    render(payload);
  }
}

function receiveFocus(payload) {
  if (payload?.focus?.kind === "step") {
    selectedStep = payload.focus.step ?? payload.focus.name;
    render(currentState);
  }
  if (payload?.focus?.kind === "pipeline") {
    requestPipeline(payload.focus.name ?? payload.focus.pipeline);
  }
}

function render(state) {
  currentState = state;
  renderHeader(state);
  renderFindings(generalFindings, state.generalFindings ?? []);
  if (state.selectedPipeline) {
    renderPipeline(state, state.selectedPipeline.name);
  } else {
    renderConfig(state);
  }
  renderDrillDown(state.selectedPipeline);
}

function renderHeader(state) {
  const counts = state.config?.view ?? { assignments: [], roles: [], repos: [], pipelines: [], orphans: [] };
  summary.textContent = `${counts.assignments.length} assignment · ${counts.roles.length} roles · ${counts.repos.length} repos · ${counts.pipelines.length} pipelines`;
  status.replaceChildren(
    textLine(state.status),
    textLine(state.validation?.dir ?? state.dir),
    textLine(`${counts.orphans.length} orphan${counts.orphans.length === 1 ? "" : "s"}`),
  );
}

function renderConfig(state) {
  const layout = state.config?.layout ?? { items: [], edges: [] };
  const byId = new Map(layout.items.map((item) => [item.id, item]));
  setSurfaceSize(layout.items);
  edgeLayer.replaceChildren(...layout.edges.map((edge) => configEdge(edge, byId)).filter(Boolean));
  cardLayer.replaceChildren(...layout.items.map((item) => configCard(state, item)));
}

function renderPipeline(state, name) {
  const pipeline = state.pipelines?.[name];
  const layout = pipeline?.layout ?? { steps: [], terminals: [], edges: [] };
  const items = [...layout.steps, ...layout.terminals];
  const byId = new Map(items.map((item) => [item.id, item]));
  setSurfaceSize(items);
  edgeLayer.replaceChildren(...pipelineEdges(layout.edges, byId));
  cardLayer.replaceChildren(
    pipelineToolbar(name),
    ...concurrentBoxes(layout.steps),
    ...layout.steps.map((step) => stepCard(state, name, step)),
    ...layout.terminals.map(terminalCard),
  );
}

function setSurfaceSize(items) {
  const width = Math.max(0, ...items.map((item) => item.x)) + CARD_WIDTH + CARD_PAD;
  const height = Math.max(0, ...items.map((item) => item.y)) + CARD_HEIGHT + CARD_PAD;
  surface.style.width = `${width}px`;
  surface.style.height = `${height}px`;
  edgeLayer.setAttribute("viewBox", `0 0 ${width} ${height}`);
  edgeLayer.setAttribute("width", width);
  edgeLayer.setAttribute("height", height);
}

function configEdge(edge, byId) {
  const source = byId.get(edge.source);
  const target = byId.get(edge.target);
  if (!source || !target) {
    return null;
  }
  return svgLine(source.x + CARD_WIDTH, source.y + CARD_HEIGHT / 2, target.x, target.y + CARD_HEIGHT / 2, "edge-path");
}

function pipelineEdges(edges, byId) {
  return edges.flatMap((edge) => {
    const source = byId.get(edge.source);
    const target = byId.get(edge.target);
    if (!source || !target) {
      return [];
    }
    const path = routePath(edge.route, source, target);
    const line = svgPath(path, `edge-path ${edgeClass(edge)}`);
    const label = edgeLabel(edge, path.midX, path.midY);
    return label ? [line, label] : [line];
  });
}

function routePath(route, source, target) {
  const start = startPoint(route, source);
  const end = endPoint(route, target);
  const midX = route === "back" ? Math.min(start.x, end.x) - CARD_PAD : (start.x + end.x) / 2;
  const midY = (start.y + end.y) / 2;
  if (route === "spine") {
    return { d: `M ${start.x} ${start.y} L ${end.x} ${end.y}`, midX, midY };
  }
  if (route === "back") {
    return { d: `M ${start.x} ${start.y} L ${midX} ${start.y} L ${midX} ${end.y} L ${end.x} ${end.y}`, midX, midY };
  }
  return { d: `M ${start.x} ${start.y} C ${midX} ${start.y}, ${midX} ${end.y}, ${end.x} ${end.y}`, midX, midY };
}

function startPoint(route, item) {
  if (route === "spine") {
    return { x: item.x + CARD_WIDTH / 2, y: item.y + CARD_HEIGHT };
  }
  if (route === "back") {
    return { x: item.x, y: item.y + CARD_HEIGHT / 2 };
  }
  return { x: item.x + CARD_WIDTH, y: item.y + CARD_HEIGHT / 2 };
}

function endPoint(route, item) {
  if (route === "spine") {
    return { x: item.x + CARD_WIDTH / 2, y: item.y };
  }
  if (route === "back") {
    return { x: item.x, y: item.y + CARD_HEIGHT / 2 };
  }
  return { x: item.x, y: item.y + CARD_HEIGHT / 2 };
}

function edgeClass(edge) {
  if (edge.relation === "data") {
    return "edge--data";
  }
  if (edge.relation === "observes") {
    return "edge--observes";
  }
  return `edge--${edge.outcome}`;
}

function edgeLabel(edge, x, y) {
  const text = edge.relation === "control" ? edge.outcome : edge.relation;
  if (!text) {
    return null;
  }
  const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
  label.setAttribute("class", "edge-label");
  label.setAttribute("x", x + 4);
  label.setAttribute("y", y - 4);
  label.textContent = text;
  return label;
}

function svgLine(x1, y1, x2, y2, className) {
  const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
  line.setAttribute("class", className);
  line.setAttribute("x1", x1);
  line.setAttribute("y1", y1);
  line.setAttribute("x2", x2);
  line.setAttribute("y2", y2);
  line.setAttribute("stroke", "currentColor");
  return line;
}

function svgPath(path, className) {
  const element = document.createElementNS("http://www.w3.org/2000/svg", "path");
  element.setAttribute("class", className);
  element.setAttribute("d", path.d);
  return element;
}

function configCard(state, item) {
  const data = dataFor(state, item);
  const card = baseCard(`card card--${item.kind}${item.orphan ? " card--orphan" : ""}`, item);
  card.dataset.ref = item.id;
  card.append(configCardBody(state, item, data));
  return card;
}

function baseCard(className, item) {
  const card = document.createElement("article");
  card.className = className;
  card.style.transform = `translate(${item.x}px, ${item.y}px)`;
  return card;
}

function configCardBody(state, item, data) {
  if (item.kind === "pipeline") {
    const button = document.createElement("button");
    button.className = "card-button";
    button.type = "button";
    button.append(...configCardContents(state, item, data));
    button.addEventListener("click", () => requestPipeline(item.name));
    return button;
  }
  const body = document.createElement("div");
  body.append(...configCardContents(state, item, data));
  return body;
}

function configCardContents(state, item, data) {
  return [
    title(item.name),
    detailFor(item, data),
    chipRow(chipsFor(state, item, data)),
    stepList(state, item),
    findingList([...(state.findingsByItem?.[item.id] ?? [])]),
  ].filter(Boolean);
}

function dataFor(state, item) {
  const view = state.config?.view ?? {};
  const sources = {
    assignment: view.assignments ?? [],
    role: view.roles ?? [],
    repo: view.repos ?? [],
    pipeline: view.pipelines ?? [],
  };
  if (item.kind === "work-source") {
    return (view.assignments ?? []).find((assignment) => `work-source:${assignment.name}` === item.id);
  }
  return (sources[item.kind] ?? []).find((value) => value.name === item.name) ?? {};
}

function detailFor(item, data) {
  const detail = document.createElement("p");
  detail.className = "detail";
  detail.textContent = detailText(item, data);
  return detail;
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
    return [data.work?.filter, approvalChip(data.work?.approvalLabel)].filter(Boolean).map(labelChip);
  }
  if (item.kind === "assignment") {
    return limitChips(data.limits ?? {});
  }
  if (item.kind === "role") {
    return (data.permissions ?? []).map((permission) => permissionChip(permission, data.usedBy ?? []));
  }
  if (item.kind === "repo") {
    return repoChips(state, data);
  }
  return pipelineChips(state, data);
}

function labelChip(text) {
  return { text };
}

function approvalChip(label) {
  return label ? `approval: ${label}` : null;
}

function limitChips(limits) {
  return Object.entries(limits)
    .filter(([, value]) => value != null)
    .map(([name, value]) => ({ text: `${name}: ${value}` }));
}

function permissionChip(permission, refs) {
  return { text: permission, className: "permission-chip", refs };
}

function repoChips(state, repo) {
  const primaries = (state.config?.view?.assignments ?? []).filter((assignment) => assignment.primaryRepo === repo.name);
  return [labelChip(repo.access), ...primaries.map((assignment) => labelChip(`primary: ${assignment.name}`))];
}

function pipelineChips(state, pipeline) {
  const pipelineSummary = state.pipelines?.[pipeline.name]?.summary ?? { kindCounts: {} };
  return Object.entries(pipelineSummary.kindCounts).map(([kind, count]) => labelChip(`${kind}×${count}`));
}

function chipRow(chips) {
  if (chips.length === 0) {
    return null;
  }
  const row = document.createElement("div");
  row.className = "chips";
  row.append(...chips.map(chipElement));
  return row;
}

function chipElement(chip) {
  const element = document.createElement("span");
  element.className = `chip ${chip.className ?? ""}`.trim();
  element.textContent = chip.text;
  if (chip.refs) {
    element.tabIndex = 0;
    element.addEventListener("pointerenter", () => setHighlights(chip.refs));
    element.addEventListener("pointerleave", clearHighlights);
    element.addEventListener("focus", () => setHighlights(chip.refs));
    element.addEventListener("blur", clearHighlights);
  }
  return element;
}

function stepList(state, item) {
  if (item.kind !== "pipeline") {
    return null;
  }
  const steps = state.pipelines?.[item.name]?.summary?.agentSteps ?? [];
  const list = document.createElement("div");
  list.className = "step-list";
  list.append(...steps.map((step) => stepBadge(state, step)));
  return list;
}

function stepBadge(state, step) {
  const badge = document.createElement("span");
  const findings = findingList(state.findingsByStep?.[step.ref] ?? []);
  badge.className = "step-badge";
  badge.dataset.ref = step.ref;
  badge.textContent = `${step.name} · ${step.role} · trust: ${step.trust ?? "role"}`;
  if (findings) {
    badge.append(findings);
  }
  return badge;
}

function pipelineToolbar(name) {
  const toolbar = document.createElement("div");
  toolbar.className = "pipeline-toolbar";
  toolbar.append(backButton(), title(name), legend());
  return toolbar;
}

function backButton() {
  const button = document.createElement("button");
  button.className = "back-button";
  button.type = "button";
  button.textContent = "Back to config";
  button.addEventListener("click", backToConfig);
  return button;
}

function legend() {
  const row = document.createElement("div");
  row.className = "legend";
  row.setAttribute("aria-label", "Edge legend");
  row.append(
    legendItem("success", "var(--bureau-teal)"),
    legendItem("failure", "var(--bureau-red)"),
    legendItem("blocked", "var(--bureau-amber)"),
    legendItem("no-work", "var(--bureau-grey)"),
    legendItem("data dashed", "var(--bureau-blue)"),
    legendItem("observes dotted", "var(--bureau-yellow)"),
  );
  return row;
}

function legendItem(text, color) {
  const item = document.createElement("span");
  const swatch = document.createElement("span");
  swatch.className = "legend-swatch";
  swatch.style.setProperty("--swatch", color);
  item.append(swatch, ` ${text}`);
  return item;
}

function concurrentBoxes(steps) {
  const byId = new Map(steps.map((step) => [step.id, step]));
  return steps.filter((step) => step.kind === "concurrent").map((step) => concurrentBox(step, byId));
}

function concurrentBox(step, byId) {
  const members = (step.fields.members ?? []).map((member) => byId.get(member)).filter(Boolean);
  const left = Math.min(step.x, ...members.map((member) => member.x)) - MEMBER_PAD;
  const top = Math.min(step.y, ...members.map((member) => member.y)) - MEMBER_PAD;
  const right = Math.max(step.x + CARD_WIDTH, ...members.map((member) => member.x + CARD_WIDTH)) + MEMBER_PAD;
  const bottom = Math.max(step.y + CARD_HEIGHT, ...members.map((member) => member.y + CARD_HEIGHT)) + MEMBER_PAD;
  const box = document.createElement("div");
  box.className = "concurrent-box";
  box.style.transform = `translate(${left}px, ${top}px)`;
  box.style.width = `${right - left}px`;
  box.style.height = `${bottom - top}px`;
  return box;
}

function stepCard(state, pipelineName, step) {
  const className = [
    "card",
    "step-card",
    `card--${step.kind}`,
    step.parentId ? "card--member" : "",
    selectedStep === step.name ? "is-highlighted" : "",
    unreachableClass(state, pipelineName, step),
  ].filter(Boolean).join(" ");
  const card = baseCard(className, step);
  const button = document.createElement("button");
  button.className = "step-button";
  button.type = "button";
  button.append(...[title(step.name), stepDetail(step), stepChips(step), findingList(state.findingsByStep?.[`pipeline:${pipelineName}/${step.name}`] ?? [])].filter(Boolean));
  button.addEventListener("click", () => selectStep(step.name));
  card.dataset.ref = `pipeline:${pipelineName}/${step.name}`;
  card.append(button);
  return card;
}

function unreachableClass(state, pipelineName, step) {
  const findings = state.findingsByStep?.[`pipeline:${pipelineName}/${step.name}`] ?? [];
  return findings.some((finding) => /unreachable/i.test(finding.message ?? "")) ? "card--unreachable" : "";
}

function stepDetail(step) {
  const line = document.createElement("p");
  line.className = "detail";
  line.textContent = stepDetailText(step);
  return line;
}

function stepDetailText(step) {
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
  const chips = [
    step.parentId ? { text: `member of ${step.parentId}` } : null,
    step.fields.trust ? { text: `trust: ${step.fields.trust}` } : null,
    step.fields.maxAttempts > 1 ? { text: `attempts: ${step.fields.maxAttempts}` } : null,
  ].filter(Boolean);
  return chipRow(chips) ?? document.createElement("span");
}

function terminalCard(terminal) {
  const card = baseCard("card step-card card--terminal", terminal);
  card.append(title(terminal.name));
  return card;
}

function findingList(findings) {
  if (!findings || findings.length === 0) {
    return null;
  }
  const list = document.createElement("div");
  list.className = "findings";
  renderFindings(list, findings);
  return list;
}

function renderFindings(container, findings) {
  container.replaceChildren(...findings.map(findingElement));
}

function findingElement(finding) {
  const element = document.createElement("span");
  const advisory = finding.marker === "advisory" || finding.source === "advisory";
  element.className = `finding ${advisory ? "finding--advisory" : "finding--validation"}`;
  element.textContent = finding.message;
  return element;
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

function selectStep(name) {
  selectedStep = name;
  for (const element of document.querySelectorAll(".step-card")) {
    element.classList.toggle("is-highlighted", element.textContent.includes(name));
  }
}

async function requestPipeline(name) {
  const result = await postIntent({ kind: "open-pipeline", pipeline: name });
  if (result?.ok) {
    render(result.state);
  }
}

async function backToConfig() {
  const result = await postIntent({ kind: "back-to-config" });
  if (result?.ok) {
    selectedStep = null;
    render(result.state);
  }
}

async function postIntent(body) {
  const response = await fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return response.ok ? response.json() : null;
}

function renderDrillDown(selected) {
  drillDown.hidden = !selected;
  drillDown.replaceChildren();
  if (selected) {
    drillDown.append(textLine(selected.missing ? `${selected.name} is not present.` : "Select a step to inspect it. No edits are available in this view."));
  }
}

function title(text) {
  const heading = document.createElement("h2");
  heading.textContent = text;
  return heading;
}

function textLine(text) {
  const line = document.createElement("p");
  line.textContent = text ?? "";
  return line;
}