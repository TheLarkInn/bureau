// The Bureau Canvas State Lab.
//
// A review surface, not a second canvas: every pixel of the rendered state
// comes from the production page in the iframe, driven through the same
// registry and the same driver the browser suite uses. The lab's own chrome is
// plain DOM on purpose — if it were built from the canvas's components it
// could start to disagree with them.

import { collect, CONTRAST, copyLabel, graphsDrawn, measureFor, selectorsFor, verdict } from "./checks.mjs";
import { CONSTRAINTS, ENTRY_TRANSITIONS, EXCLUSIONS, ORDER, rootReason, STATES, summary, TRANSITIONS } from "./registry.mjs";
import { DIMENSION_BY_ID } from "./dimensions.mjs";
import { violations } from "./constraints.mjs";
import { domAdapter } from "./dom-adapter.mjs";
import { servableInFrame } from "./intercept.mjs";
import { runPath } from "./driver.mjs";
import { FIXTURES, describeFixture, SAMPLE_ASSIGNMENT } from "./fixtures.mjs";
import { VIEWPORTS } from "./selectors.mjs";

const frame = document.querySelector("#stage-frame");
const stage = document.querySelector("#stage");
let base = null;
let current = STATES[0];
let viewport = "desktop";
/** Serialises walks: one iframe, one entry path at a time. */
let queue = Promise.resolve();

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  if (text != null) {
    node.textContent = text;
  }
  return node;
};

async function boot() {
  base = await fetch("./state", { cache: "no-store" }).then((response) => response.json());
  renderBaseNote();
  renderSummary();
  renderList();
  renderDimensions();
  renderConstraints();
  renderPicker();
  await show(stateFromHash() ?? STATES[0]);
  window.addEventListener("hashchange", () => {
    const next = stateFromHash();
    if (next && next.id !== current.id) {
      void show(next);
    }
  });
}

/**
 * Every fixture is a *transform* of whatever the host serves, so the renders
 * are only the ones CI asserts when the host is serving the bundled sample —
 * which it is under `BUREAU_CANVAS_TEST=1`, and is not when a contributor
 * opens the lab against their own `.bureau/`. Same state id, different content.
 *
 * The lab cannot fix that from here without ceasing to be the production page
 * reading its real host. What it can do is refuse to let a reviewer read their
 * own config as the fixture, so a state that looks wrong is not blamed on the
 * UI when it is the payload underneath that differs.
 */
function renderBaseNote() {
  const note = document.querySelector("#base-note");
  const missing = expectedByFixtures();
  note.hidden = missing.length === 0;
  if (missing.length) {
    note.textContent = `This host is not serving the bundled sample (${missing.join("; ")}). Fixtures are applied over whatever it serves, so these renders are your own config in the shape of each state — not the payload the browser suite asserts.`;
  }
}

/** What the fixture transforms assume the served payload already contains. */
function expectedByFixtures() {
  const view = base?.config?.view ?? {};
  const sample = (view.assignments ?? []).find((item) => item.name === SAMPLE_ASSIGNMENT);
  const pipelines = (view.pipelines ?? []).map((item) => item.name);
  return [
    sample ? null : `no assignment called \`${SAMPLE_ASSIGNMENT}\``,
    !sample || pipelines.includes(sample.pipeline) ? null : `it names a pipeline (\`${sample.pipeline}\`) this config does not define`,
  ].filter(Boolean);
}

function stateFromHash() {
  const id = decodeURIComponent(window.location.hash.replace(/^#/u, ""));
  return STATES.find((state) => state.id === id) ?? null;
}

function renderSummary() {
  const counts = summary();
  const bar = document.querySelector("#summary");
  bar.replaceChildren(...Object.entries(counts).map(([key, value]) => {
    const item = el("span", "metric");
    item.append(el("strong", null, String(value)), el("span", null, camel(key)));
    return item;
  }));
}

function camel(key) {
  return key.replace(/([A-Z])/gu, " $1").toLowerCase();
}

function renderList() {
  const list = document.querySelector("#states");
  const search = document.querySelector("#search");
  const draw = () => {
    const term = search.value.trim().toLowerCase();
    const groups = new Map();
    for (const state of STATES) {
      if (term && !`${state.id} ${state.summary ?? ""}`.toLowerCase().includes(term)) {
        continue;
      }
      groups.set(state.surface, [...(groups.get(state.surface) ?? []), state]);
    }
    list.replaceChildren(...[...groups].flatMap(([surface, states]) => [
      el("h3", "group", `${surface} · ${states.length}`),
      ...states.map(stateButton),
    ]));
  };
  search.addEventListener("input", draw);
  draw();
}

function stateButton(state) {
  const button = el("button", `state-item${state.id === current.id ? " is-active" : ""}`);
  button.type = "button";
  button.append(el("span", "state-id", state.id), el("span", "state-note", state.summary ?? ""));
  if (state.kind === "probe") {
    button.append(el("span", "tag tag--probe", "probe"));
  }
  if (state.intercept) {
    button.append(el("span", "tag tag--intercept", state.intercept));
  }
  button.addEventListener("click", () => show(state));
  return button;
}

/**
 * Selecting a state walks its entry path against the frame. Two walks at once
 * would fight over one iframe — a viewport switch mid-walk used to reload the
 * page underneath the walk in flight — so they are serialised on a promise
 * queue and each walk runs to completion before the next begins.
 */
function show(state) {
  queue = queue.catch(() => {}).then(() => walk(state));
  return queue;
}

async function walk(state) {
  current = state;
  window.location.hash = encodeURIComponent(state.id);
  for (const node of document.querySelectorAll(".state-item")) {
    node.classList.toggle("is-active", node.querySelector(".state-id")?.textContent === state.id);
  }
  applyViewport();
  if (state.intercept && !servableInFrame(state.intercept)) {
    // Clearing the frame is the point. Returning early used to leave the
    // *previous* state's render on screen beside this state's description, so
    // the lab presented a screen it had not produced as though it had — the
    // one failure mode a review surface may not have. There is nothing
    // truthful to draw here, so it draws nothing and says why.
    blankFrame();
    renderDetail(state, { intercepted: true });
    return;
  }
  // A pending entry path is not a failing one. Saying so beats showing a wall
  // of red crosses that only means "the walk has not finished yet".
  renderDetail(state, { pending: true });
  try {
    const adapter = domAdapter(frame);
    await runPath(state.ops, adapter, base);
    renderDetail(state, { ...(await settledInspect(state)), channel: adapter.channel });
  } catch (error) {
    renderDetail(state, { failed: String(error?.message ?? error) });
  }
}

const SETTLE_MS = 2000;
const SETTLE_POLL_MS = 50;

/**
 * Judges the render once it has settled.
 *
 * React commits asynchronously, so a verdict taken the instant the last click
 * returns can catch the DOM mid-update — on a loaded machine that reads as a
 * state failing its own registry. The browser suite takes its verdict through
 * a single `page.evaluate` rather than a locator, so nothing there retries
 * either; `matrix-fixtures.mjs` runs the same loop for the same reason. Both
 * report whatever the last look found, so a genuinely wrong state still fails.
 *
 * Leaving on the first failure-free look was not enough, and this surface is
 * the one where that mattered most. No expectation names a relation graph's
 * *edges* — React Flow draws them in a pass after it has measured the nodes —
 * so the loop could exit during the lull between the two, and the panel would
 * report "no overlap, clipping, low contrast…" beside a picture of steps
 * joined to nothing. That is the review surface vouching for a frame, which is
 * exactly the fault `settled` was introduced in the matrix to stop; the lab
 * making the opposite claim about the same registry is the contradiction.
 *
 * So the graph barrier is shared with the matrix, and whether it was ever
 * reached is returned rather than kept: a render the loop could not prove is
 * marked, not presented as verified.
 */
async function settledInspect(state) {
  const deadline = Date.now() + SETTLE_MS;
  let result = inspect(state);
  while ((result.failures.length || !graphsDrawn(result.snapshot)) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, SETTLE_POLL_MS));
    result = inspect(state);
  }
  return { ...result, settled: graphsDrawn(result.snapshot) };
}

function inspect(state) {
  const snapshot = collect(frame.contentDocument, { selectors: selectorsFor(state), measure: measureFor(state), contrast: CONTRAST });
  return { snapshot, failures: verdict(state, snapshot, { slack: 2 }) };
}

/**
 * Empties the stage, so nothing a previous walk drew can be read as this
 * state's render. `about:blank` rather than hiding the frame: a hidden frame
 * keeps its document, and the next walk would inherit it.
 */
function blankFrame() {
  frame.src = "about:blank";
}

/** The file the browser suite writes this state's render to, at each viewport. */
function galleryShot(state) {
  return `${viewport}--${state.id.replace(/[^a-z0-9]+/giu, "_")}.png`;
}

function renderDetail(state, result) {
  const panel = document.querySelector("#detail");
  panel.replaceChildren(
    el("h2", null, state.id),
    el("p", "muted", state.summary ?? ""),
    dimensionTable(state),
    pathList(state),
    expectationList(state, result),
    transitionList(state),
  );
}

function dimensionTable(state) {
  const box = el("section", "panel");
  box.append(el("h3", null, "Dimensions"));
  const list = el("dl", "kv");
  for (const key of ORDER) {
    const value = state.dimensions?.[key];
    if (!value || value === "n/a") {
      continue;
    }
    list.append(el("dt", null, key), el("dd", null, value));
  }
  if (state.rule) {
    list.append(el("dt", null, "crossing excluded by"), el("dd", null, state.rule));
  }
  if (state.covers) {
    list.append(el("dt", null, "covers"), el("dd", null, state.covers));
  }
  list.append(el("dt", null, "fixture"), el("dd", null, describeFixture(state.fixture)));
  box.append(list);
  return box;
}

function pathList(state) {
  const box = el("section", "panel");
  box.append(el("h3", null, "Entry path"));
  const list = el("ol", "ops");
  for (const op of state.ops) {
    list.append(el("li", null, describeOp(op)));
  }
  box.append(list);
  return box;
}

function describeOp(op) {
  if (op.op === "page") {
    return `load ${op.value}.html${op.intercept ? ` (${op.intercept})` : ""}`;
  }
  if (op.op === "fixture") {
    return `publish fixture ${[].concat(op.value).join(" + ") || "none"}`;
  }
  if (op.op === "fill" || op.op === "select") {
    return `${op.op} ${op.selector} = ${JSON.stringify(op.value)}`;
  }
  if (op.op === "drag") {
    return `drag ${op.selector} by ${op.dx},${op.dy}`;
  }
  return `${op.op} ${op.selector ?? ""}`.trim();
}

function expectationList(state, result) {
  const box = el("section", "panel");
  box.append(el("h3", null, "Expected vs rendered"));
  if (result?.pending) {
    box.append(el("p", "note", "walking the entry path…"));
    return box;
  }
  if (result?.intercepted) {
    box.append(el("p", "note note--warn", `Needs the renderer module blocked (${state.intercept}). A \`<script type="module">\` is not fetched through \`window.fetch\`, so this is the one condition the lab cannot install from inside the frame. The stage is left blank on purpose rather than showing the previous state's render.`));
    box.append(el("p", "note", `specs/state-matrix.spec.mjs renders this state and writes it to the gallery as ${galleryShot(state)}.`));
    return box;
  }
  if (result?.failed) {
    box.append(el("p", "note note--err", result.failed));
    return box;
  }
  const list = el("ul", "expectations");
  for (const selector of state.expect.shows) {
    list.append(row(selector, "visible", (result?.snapshot.counts[selector] ?? 0) > 0));
  }
  for (const selector of state.expect.hides) {
    list.append(row(selector, "absent", (result?.snapshot.counts[selector] ?? 0) === 0));
  }
  for (const phrase of state.expect.copy) {
    const label = copyLabel(phrase);
    const ok = !(result?.failures ?? []).some((item) => item.kind === "missing-copy" && item.detail === label);
    list.append(row(typeof phrase === "object" && phrase !== null ? label : `“${label}”`, "copy", ok));
  }
  box.append(list);
  // The SSE barrier is best-effort by design; when it does not arm, the host's
  // own payload may still have landed after the fixture. Saying so is the
  // difference between a review tool and a reassuring one.
  if (result?.channel?.observed === false) {
    box.append(el("p", "note note--warn", `Not proved settled: ${result.channel.reason}. This render may have raced the host's own payload.`));
  }
  // The graph half of the same claim. A relation or pipeline surface that never
  // finished its edge pass is a picture of steps joined to nothing, and every
  // other line in this panel would still read green — none of them names an
  // edge. Marked rather than failed, for the reason the matrix marks it: the
  // lab renders one state at a time in a live frame, so a slow pass here is
  // ordinary, and the panel's job is to say which renders may be believed.
  if (result?.settled === false) {
    box.append(el("p", "note note--warn", "Not proved settled: a graph on this render had not drawn all of its edges. Re-run this state before reading its graph."));
  }
  const layout = (result?.failures ?? []).filter((item) => ["overlap", "clipped", "horizontal-overflow", "low-contrast", "placeholder-copy"].includes(item.kind));
  box.append(el("p", layout.length ? "note note--err" : "note", layout.length
    ? layout.map((item) => `${item.kind}: ${item.detail}`).join("; ")
    : "no overlap, clipping, low contrast, placeholder copy or horizontal overflow"));
  return box;
}

function row(label, kind, ok) {
  const item = el("li", ok ? "ok" : "bad");
  item.append(el("span", "verdict", ok ? "✓" : "✗"), el("code", null, label), el("span", "muted", kind));
  return item;
}

function transitionList(state) {
  const box = el("section", "panel");
  const inbound = TRANSITIONS.filter((edge) => edge.to === state.id);
  const outbound = TRANSITIONS.filter((edge) => edge.from === state.id);
  box.append(el("h3", null, `Transitions (${inbound.length} in · ${outbound.length} out)`));
  // The note is owed to a state nothing *enters*, which is not the same as a
  // state with no edges at all. Gating on the whole list meant a root that
  // opens something — a landing with three ways out — silently lost its note,
  // and those are the states a reviewer is most likely to be looking at.
  if (!ENTRY_TRANSITIONS.some((edge) => edge.to === state.id)) {
    box.append(rootNote(state, inbound.length));
  }
  const list = el("ul", "edges");
  for (const edge of inbound) {
    list.append(edgeRow("←", edge.from, edge.via));
  }
  for (const edge of outbound) {
    list.append(edgeRow("→", edge.to, edge.via));
  }
  if (list.childElementCount) {
    box.append(list);
  }
  return box;
}

/**
 * What a state nothing reaches first says for itself.
 *
 * The panel used to print "a root of the DAG" for all of them — the one
 * sentence that is true of every root and so distinguishes none of them. A
 * reviewer looking at such a state needs to know which kind it is, because
 * "this is where the canvas opens" and "the screen above this one is not a
 * state" are opposite findings.
 */
function rootNote(state, inbound) {
  const reason = rootReason(state);
  const box = el("div", "root");
  box.dataset.rootReason = reason.id;
  box.append(el("p", null, `Nothing reaches this screen first — ${reason.title}.`), el("p", "muted", reason.why));
  if (inbound) {
    box.append(el("p", "muted", `The ${inbound} edge(s) arriving here are ways back out of screens this one opens, so a reader is already here before taking one. That is not a first arrival, which is why this is still a root.`));
  }
  return box;
}

function edgeRow(arrow, id, via) {
  const item = el("li");
  const link = el("button", "linkish", `${arrow} ${id}`);
  link.type = "button";
  link.addEventListener("click", () => show(STATES.find((state) => state.id === id)));
  item.append(link, el("span", "muted", via));
  return item;
}

function renderDimensions() {
  const box = document.querySelector("#dimensions");
  box.replaceChildren(...Object.values(DIMENSION_BY_ID).map((dimension) => {
    const item = el("details", "panel");
    item.append(el("summary", null, `${dimension.title} — ${dimension.values.length} values`));
    item.append(el("p", "muted", dimension.why));
    const list = el("ul", "values");
    for (const value of dimension.values) {
      const row = el("li");
      row.append(el("code", null, value.id), el("span", "muted", value.summary ?? ""));
      list.append(row);
    }
    item.append(list);
    return item;
  }));
}

function renderConstraints() {
  const box = document.querySelector("#constraints");
  const counts = Object.fromEntries(EXCLUSIONS.map((entry) => [entry.rule, entry]));
  box.replaceChildren(...CONSTRAINTS.map((rule) => {
    const item = el("details", "panel");
    const pruned = counts[rule.id]?.pruned ?? 0;
    item.append(el("summary", null, `${rule.title} — ${pruned.toLocaleString()} pruned here`));
    item.append(el("p", "muted", `${rule.kind} · reads ${rule.reads.join(", ")}`));
    item.append(el("p", null, rule.why));
    if (rule.kind === "harness") {
      // A harness rule hides a screen a user really reaches, so it owes the
      // reviewer both halves: what stops the harness, and where the same screen
      // is rendered instead.
      item.append(el("p", "note", `Harness limit — ${rule.limit}`));
      item.append(el("p", "note", `The same screen is rendered by ${rule.stands}.`));
    }
    item.append(el("p", "muted", "“Pruned here” counts the tuples this rule was the first to reject, in walk order — not every tuple it forbids. Ask the picker below about a specific combination to see every rule that rejects it."));
    const example = counts[rule.id]?.example;
    if (example) {
      const assigned = Object.entries(example.assigned).map(([key, value]) => `${key}=${value}`).join("\n");
      item.append(el("pre", "example", `${assigned}\n… and every value of the remaining ${example.of - example.depth} dimension(s)`));
    }
    return item;
  }));
}

/**
 * The picker answers the order-free question the per-rule counts cannot: for
 * one combination a reviewer chooses, is it a state, and if not, which rules
 * reject it — all of them, not just the one that happened to prune first.
 *
 * It is built from `ORDER` and `DIMENSION_BY_ID`, so a dimension or value
 * added to the registry appears here without anyone editing this function.
 */
function renderPicker() {
  const box = document.querySelector("#picker");
  const selects = new Map();
  for (const key of ORDER) {
    const row = el("label", "picker-row");
    row.append(el("span", "picker-key", key));
    const select = document.createElement("select");
    select.setAttribute("aria-label", key);
    for (const value of DIMENSION_BY_ID[key].values) {
      const option = document.createElement("option");
      option.value = value.id;
      option.textContent = value.id;
      select.append(option);
    }
    select.addEventListener("change", judge);
    selects.set(key, select);
    row.append(select);
    box.append(row);
  }
  document.querySelector("#picker-verdict").dataset.ready = "true";
  seed();

  /**
   * Open on a state rather than on whatever value happens to sit first on each
   * axis. Every optional dimension leads with `n/a`, so the unseeded default
   * was a combination no rule allows — a picker that opens on "not a state"
   * teaches a reviewer nothing about the axes it is offering.
   */
  function seed() {
    const start = STATES.find((state) => state.kind === "matrix")?.dimensions ?? {};
    for (const [key, select] of selects) {
      if (start[key] !== undefined) {
        select.value = start[key];
      }
    }
  }

  function judge() {
    const combo = Object.fromEntries([...selects].map(([key, select]) => [key, select.value]));
    const broken = violations(combo);
    const match = STATES.find((state) => ORDER.every((key) => state.dimensions?.[key] === combo[key]));
    render(combo, broken, match);
  }

  function render(combo, broken, match) {
    const panel = document.querySelector("#picker-verdict");
    panel.dataset.verdict = broken.length ? "excluded" : "reachable";
    const parts = [el("p", "picker-headline", broken.length
      ? `Not a state — ${broken.length} rule${broken.length === 1 ? "" : "s"} reject this combination.`
      : match ? `Reachable, and enumerated as ${match.id}.` : "Reachable: no rule rejects it.")];
    for (const id of broken) {
      const rule = CONSTRAINTS.find((item) => item.id === id);
      const entry = el("details");
      entry.append(el("summary", null, `${rule.id} · ${rule.kind}`), el("p", null, rule.why));
      if (rule.kind === "harness") {
        entry.append(el("p", "note", `Harness limit — ${rule.limit}`), el("p", "note", `The same screen is rendered by ${rule.stands}.`));
      }
      parts.push(entry);
    }
    if (!broken.length && !match) {
      parts.push(el("p", "muted", "No rule rejects it and no enumerated state carries it — that is a registry gap worth reporting."));
    }
    panel.replaceChildren(...parts);
  }

  judge();
}

function applyViewport() {
  const size = VIEWPORTS[viewport];
  stage.style.width = `${size.width}px`;
  stage.style.height = `${size.height}px`;
  document.querySelector("#viewport-note").textContent = `${size.width}×${size.height} — ${size.summary}`;
}

for (const button of document.querySelectorAll("[data-viewport]")) {
  button.addEventListener("click", () => {
    viewport = button.dataset.viewport;
    for (const other of document.querySelectorAll("[data-viewport]")) {
      other.classList.toggle("is-active", other === button);
    }
    void show(current);
  });
}

document.querySelector("#reload").addEventListener("click", () => show(current));

await boot();
