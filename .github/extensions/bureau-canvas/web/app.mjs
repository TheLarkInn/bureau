const CARD_WIDTH = 216;
const CARD_HEIGHT = 112;
const CARD_PAD = 64;

const summary = document.querySelector("#summary");
const status = document.querySelector("#status");
const surface = document.querySelector("#config-view");
const edgeLayer = document.querySelector("#edges");
const cardLayer = document.querySelector("#cards");
const drillDown = document.querySelector("#drill-down");
const generalFindings = document.querySelector("#general-findings");

let currentState = null;

await loadState();

const events = new EventSource("./events");
events.addEventListener("state", (event) => render(JSON.parse(event.data)));

async function loadState() {
  const response = await fetch("./state", { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`state request failed: ${response.status}`);
  }
  render(await response.json());
}

function render(state) {
  currentState = state;
  renderHeader(state);
  renderFindings(generalFindings, state.generalFindings ?? []);
  renderCards(state);
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

function renderCards(state) {
  const layout = state.config?.layout ?? { items: [], edges: [] };
  const byId = new Map(layout.items.map((item) => [item.id, item]));
  setSurfaceSize(layout.items);
  edgeLayer.replaceChildren(...layout.edges.map((edge) => edgeLine(edge, byId)).filter(Boolean));
  cardLayer.replaceChildren(...layout.items.map((item) => cardFor(state, item)));
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

function edgeLine(edge, byId) {
  const source = byId.get(edge.source);
  const target = byId.get(edge.target);
  if (!source || !target) {
    return null;
  }
  const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
  line.setAttribute("x1", source.x + CARD_WIDTH);
  line.setAttribute("y1", source.y + CARD_HEIGHT / 2);
  line.setAttribute("x2", target.x);
  line.setAttribute("y2", target.y + CARD_HEIGHT / 2);
  line.setAttribute("stroke", "currentColor");
  line.setAttribute("stroke-width", "2");
  return line;
}

function cardFor(state, item) {
  const data = dataFor(state, item);
  const card = document.createElement("article");
  card.className = `card card--${item.kind}${item.orphan ? " card--orphan" : ""}`;
  card.style.transform = `translate(${item.x}px, ${item.y}px)`;
  card.dataset.ref = item.id;
  card.append(cardBody(state, item, data));
  return card;
}

function cardBody(state, item, data) {
  if (item.kind === "pipeline") {
    const button = document.createElement("button");
    button.className = "card-button";
    button.type = "button";
    button.append(...cardContents(state, item, data));
    button.addEventListener("click", () => requestPipeline(item.name));
    return button;
  }
  const body = document.createElement("div");
  body.append(...cardContents(state, item, data));
  return body;
}

function cardContents(state, item, data) {
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
  const summary = state.pipelines?.[pipeline.name]?.summary ?? { kindCounts: {} };
  return Object.entries(summary.kindCounts).map(([kind, count]) => labelChip(`${kind}×${count}`));
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

function findingList(findings) {
  if (findings.length === 0) {
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

async function requestPipeline(name) {
  const response = await fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "open-pipeline", pipeline: name }),
  });
  if (response.ok) {
    const result = await response.json();
    render(result.state);
  }
}

function renderDrillDown(selected) {
  drillDown.hidden = !selected;
  drillDown.replaceChildren();
  if (selected) {
    drillDown.append(title(selected.name), textLine(selected.placeholder));
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