// The state matrix: the Cartesian product of every dimension value, filtered
// by the named rules in `constraints.mjs`, with an entry path and an
// expectation set attached to each survivor.
//
// Pure and deterministic. The lab, the offline tests and the browser suite all
// read this module; none of them keeps its own list of states, so a state that
// is not here is not rendered, screenshotted or asserted anywhere.

import { CONSTRAINTS } from "./constraints.mjs";
import { DIMENSIONS, valueOf, valuesOf } from "./dimensions.mjs";
import { isAction } from "./driver.mjs";
import { enumerate } from "./enumerate.mjs";
import { draftOps, EDIT_PATHS, FIELD_LIFECYCLE, fixtureFor, interceptFor, runHoldOps, runOps, runRefusalOps, selectStep } from "./paths.mjs";
import { PROBES } from "./probes.mjs";
import { SELECTORS as S } from "./selectors.mjs";

const ORDER = ["surface", "data", "draft", "section", "orphans", "disclosure", "card", "field", "fieldState", "fieldPair", "mode", "run", "transport", "tab", "pick", "edit"];

/** A stable, readable id: only the axes that carry information appear. */
function identify(combo) {
  const parts = ORDER.filter((key) => !["n/a", "none", "rest"].includes(combo[key])).map((key) => `${key}:${combo[key]}`);
  return parts.length ? parts.join("+") : "empty";
}

function pageFor(combo) {
  return valueOf("surface", combo.surface).page ?? "index";
}

/** Merges the `shows` / `hides` / `copy` each dimension value contributes. */
/**
 * Merges the `shows` / `hides` / `copy` each dimension value contributes.
 * A value may scope its expectations with `only`, for regions that a given
 * surface simply does not draw.
 */
function expectations(combo) {
  const bag = { shows: new Set(), hides: new Set(), copy: new Set(), allowErrors: new Set(), allowPlaceholder: new Set(), allowOverlap: new Set() };
  const absorb = (source) => {
    for (const [key, set] of Object.entries(bag)) {
      for (const item of source?.[key] ?? []) {
        set.add(item);
      }
    }
  };
  const suppressed = new Set(ORDER.flatMap((key) => valueOf(key, combo[key])?.suppress ?? []));
  for (const key of ORDER) {
    const value = valueOf(key, combo[key]);
    if (suppressed.has(key) || (value?.only && !value.only.includes(combo.surface))) {
      continue;
    }
    absorb(value);
    absorb(value?.derive?.(combo));
  }
  // The lifecycle entry is a contributor like any dimension value: it says what
  // this field's own draft looks like, in the treatment class as well as the
  // words, so a refusal rendered as ordinary advice fails here.
  absorb(FIELD_LIFECYCLE[combo.field]?.[combo.fieldState]);
  return {
    shows: [...bag.shows].filter((item) => !bag.hides.has(item)),
    hides: [...bag.hides],
    copy: [...bag.copy],
    allowErrors: [...bag.allowErrors],
    allowPlaceholder: [...bag.allowPlaceholder],
    allowOverlap: [...bag.allowOverlap],
    settles: settlement(combo),
  };
}

/**
 * Whether this state's render is required to come to rest.
 *
 * True for all but one value in the whole matrix, and it is a claim in both
 * directions rather than a tolerance: see `transport:playing` in
 * `dimensions.mjs` for why an exemption that only permitted instability would
 * be a mark that cannot fail. A dimension value declaring `settles: false`
 * makes the state's own motion the thing under test.
 */
function settlement(combo) {
  return !ORDER.some((key) => valueOf(key, combo[key])?.settles === false);
}

/**
 * The pre-surface states are the only ones a click cannot reach: one needs the
 * renderer module blocked, the other needs `/state` held open. Both are request
 * conditions rather than paths, which is why they ride on the `page` op.
 *
 * The suite applies them with `page.route`; the lab installs them inside the
 * frame (`intercept.mjs`) before the page's modules run. A held payload it can
 * stage itself. A blocked renderer it cannot — a `<script type="module">` is
 * not fetched through `window.fetch` — so for that one the lab blanks its stage
 * and names the reason rather than leaving the previous state's render on
 * screen, and the suite is where it is rendered and captured.
 *
 * Each page boots itself, so each has both, and both now answer a blocked
 * renderer with the same fallback shell — index.html for the config it still
 * holds, editor.html for the pipeline it was opening plus the way back.
 */
function bootOps(combo) {
  const page = pageFor(combo);
  if (combo.data !== "render-error") {
    return [{ op: "page", value: page, intercept: "stall-state" }, { op: "wait", selector: S.loading }];
  }
  const intercept = page === "editor" ? "block-editor-renderer" : "block-renderer";
  return [{ op: "page", value: page, intercept }, { op: "wait", selector: S.fallback }];
}

function configOps(combo) {
  const ops = [];
  for (const key of ["section", "orphans", "disclosure", "card", "field"]) {
    ops.push(...(valueOf(key, combo[key])?.enter ?? []));
  }
  ops.push(...(FIELD_LIFECYCLE[combo.field]?.[combo.fieldState]?.ops ?? []));
  return ops;
}

function pipelineOps(combo) {
  if (combo.run.startsWith("refused")) {
    return [...(valueOf("mode", combo.mode)?.enter ?? []), ...runRefusalOps(combo.run)];
  }
  if (combo.run.startsWith("holding")) {
    return [...(valueOf("mode", combo.mode)?.enter ?? []), ...runHoldOps(combo.run)];
  }
  return [
    ...(valueOf("mode", combo.mode)?.enter ?? []),
    ...runOps(combo.mode, combo.run),
    ...(valueOf("transport", combo.transport)?.enter ?? []),
  ];
}

function editorOps(combo) {
  const ops = [...(valueOf("tab", combo.tab)?.enter ?? [])];
  if (combo.tab !== "pipeline" || combo.pick === "none") {
    return ops;
  }
  if (combo.edit === "rest") {
    return [...ops, ...selectStep(combo.pick)];
  }
  return [...ops, ...EDIT_PATHS[combo.edit](combo.pick)];
}

/**
 * The request interception a state needs in place before its page loads.
 *
 * A save state is not something a user navigates *into* from the state next to
 * it — it is the same screen under a host that is slow or refusing, which is a
 * condition of the environment rather than a click. So it rides on the `page`
 * op the way the two pre-surface states do, and for the same reason: what is
 * being varied is the network, not the path.
 *
 * That is also why these states have no incoming edge. A page already loaded
 * without the route in place cannot acquire it, so claiming an edge into them
 * would be claiming a transition the suite could not walk.
 *
 * `paths.mjs` owns which axes ask for a route; this only places it.
 */
function interceptOp(combo) {
  const [kind] = interceptFor(combo);
  return kind ? { intercept: kind } : {};
}

/** The full entry path: load a page, publish a fixture, then act like a user. */
function entryPath(combo) {
  if (combo.surface === "boot" || combo.surface === "boot-editor") {
    return bootOps(combo);
  }
  const ops = [{ op: "page", value: pageFor(combo), ...interceptOp(combo) }, { op: "fixture", value: fixtureFor(combo) }];
  const bySurface = {
    config: () => [{ op: "wait", selector: S.configView }, ...configOps(combo)],
    pipeline: () => [{ op: "wait", selector: S.pipelineView }, ...pipelineOps(combo)],
    editor: () => [{ op: "wait", selector: S.editorTabs }, ...editorOps(combo)],
  };
  // The surface's own `enter`, between its shell wait and the body's path. It
  // was declared and read by nobody: `configOps`, `pipelineOps` and `editorOps`
  // consume the axes *below* the surface, and nothing consumed the surface
  // itself, so the pipeline surface's settle wait was dropped from all 480
  // renders and `S.liveCountSettled` was a dead constant. The comment that
  // declared it said the settle happened "once, on the surface all three modes
  // are entered from", and it happened nowhere.
  const surfaceEnter = valueOf("surface", combo.surface)?.enter ?? [];
  const [shellWait, ...body] = bySurface[combo.surface]();
  // The draft bar sits above the body on both index surfaces, so its own save
  // is walked last — after the body has settled into whatever rest it is in.
  return [...ops, shellWait, ...surfaceEnter, ...body, ...draftOps(combo.draft)];
}

function summarize(combo) {
  return ORDER.filter((key) => combo[key] !== "n/a").map((key) => `${key}=${combo[key]}`).join(" · ");
}

function toState(combo) {
  const ops = entryPath(combo);
  return {
    id: identify(combo),
    kind: "matrix",
    dimensions: { ...combo },
    surface: combo.surface,
    page: pageFor(combo),
    fixture: fixtureFor(combo),
    summary: summarize(combo),
    ops,
    intercept: ops.find((op) => op.intercept)?.intercept ?? null,
    expect: expectations(combo),
  };
}

const walked = enumerate(ORDER, valuesOf);

const matrixStates = walked.kept.map(toState);

/** Per-rule prune accounting: how many tuples each rule was first to reject. */
export const EXCLUSIONS = walked.excluded.map((entry) => ({
  ...entry,
  ...pick(CONSTRAINTS.find((rule) => rule.id === entry.rule), ["kind", "title", "why", "limit", "stands"]),
}));

function pick(source, keys) {
  return Object.fromEntries(keys.map((key) => [key, source?.[key]]));
}

/** Reachable states: the matrix plus the deliberate crossing probes. */
export const STATES = [...matrixStates, ...PROBES];

/**
 * Pairs of states that draw the same screen on purpose, and why.
 *
 * Two states rendering identically is normally a defect this registry exists to
 * catch — an entry operation that has quietly become a no-op, a control that
 * stopped varying — and until the gallery was audited nothing could notice it.
 * But a few of these identities are the very claim a probe is making: a refusal
 * that leaves with the run it was about leaves the screen it found. Saying so
 * here turns an unremarked coincidence into an assertion in both directions —
 * `e2e/playwright/gallery-audit.mjs` reports a declared twin that stops
 * matching exactly as loudly as an undeclared pair that starts, and
 * `specs/gallery.audit.spec.mjs` fails the run on either.
 *
 * `viewports` is per-pair rather than assumed, because the two layouts are two
 * screens: a claim that holds in one column may not hold in two.
 */
export const RENDER_TWINS = [
  {
    a: "probe--create-refusal-dismissed",
    b: "surface:config+data:validated+section:stack+disclosure:create+card:collapsed",
    viewports: ["desktop", "compact"],
    why: "a dismissed create refusal must leave nothing behind, so reopening the form is the form",
  },
  {
    a: "probe--run-refusal-dismissed",
    b: "probe--step-log-idle",
    viewports: ["desktop", "compact"],
    why: "the refusal leaves with the run it was about, back to Live with nothing chosen",
  },
  {
    a: "probe--selection-behind-relations-tab",
    b: "surface:editor+tab:relations",
    viewports: ["desktop", "compact"],
    why: "a step selected on the Pipeline tab may not leak onto the relation graph",
  },
  {
    a: "probe--dirty-editor-behind-relations-tab",
    b: "surface:editor+tab:relations",
    viewports: ["desktop", "compact"],
    why: "an unsaved rename is held, not shown: the relation graph draws the config, not the draft",
  },
  {
    a: "probe--dirty-editor-behind-relations-tab",
    b: "probe--selection-behind-relations-tab",
    viewports: ["desktop", "compact"],
    why: "both hide what the Pipeline tab was holding, so behind Relations they are one screen",
  },
  {
    a: "probe--draft-survives-a-tab-round-trip",
    b: "surface:editor+tab:pipeline+pick:deterministic+edit:renamed",
    viewports: ["desktop", "compact"],
    why: "the round trip is the claim: coming back must land on the screen that was left",
  },
  {
    a: "probe--draft-save-transport-lost",
    b: "surface:config+data:fixture+draft:save-error+section:stack+card:collapsed",
    viewports: ["desktop", "compact"],
    why: "a host that refused and a host that vanished are one sentence to the reader — the save did not happen, both controls are back",
  },
];

/**
 * The controls that really are toggles, and what undoes each one.
 *
 * A prefix DAG can only say how a state is *entered*: every edge points away
 * from the landing, one operation at a time. But half of what a user does is
 * the other direction — collapse the card, cancel the create, go back to the
 * Pipeline tab, leave replay for design — and none of it was under test,
 * because no state's entry path contains it. A disclosure that opens and will
 * not close is exactly the defect a state matrix exists to catch, and it could
 * not have failed anything here.
 *
 * So a reversible step declares its undo. The suite enters the child, applies
 * only that undo, and then holds the render to the *parent's* expectations,
 * which is precisely the claim being made: this control puts the page back.
 * A control that opens but does not close fails by name, and so does one that
 * closes onto a screen that is not the one it left.
 *
 * `undo` is usually the same control — these are toggles — but not always: the
 * way out of the Relations tab is the Pipeline tab, and the way out of a create
 * bar is Cancel. Both are the real control a user would reach for.
 *
 * Each entry names the region the toggle moves, and which way it moves it.
 * `gone` is a region `via` reveals, so the way back is over when it has gone;
 * `hidden` is a region `via` takes away — the group fold is the one of those,
 * because a finished group draws its members until it is asked not to — so the
 * way back is over when it is on screen again. The direction is not cosmetic: a
 * `waitGone` on a selector that never matches passes instantly, so a restoring
 * undo declared as a removing one would be an edge that asserts nothing.
 */
export const REVERSIBLE = [
  { via: S.assignmentHead, undo: S.assignmentHead, gone: S.assignmentDetail },
  { via: S.createOpen, undo: S.createCancel, gone: S.createBar },
  { via: S.relationSummary, undo: S.relationSummary, gone: S.relationOpen },
  { via: S.workSourceValue, undo: S.workSourceValue, gone: S.workSourceEditor },
  { via: S.workRulesValue, undo: S.workRulesValue, gone: S.workRulesEditor },
  { via: S.signalsValue, undo: S.signalsValue, gone: S.signalsEditor },
  { via: S.reposValue, undo: S.reposValue, gone: S.reposEditor },
  { via: S.limitsValue, undo: S.limitsValue, gone: S.limitsEditor },
  { via: S.editorTabRelations, undo: S.editorTabPipeline, gone: S.relationFlow },
  { via: S.modeLive, undo: S.modeDesign, gone: S.runControls },
  { via: S.modeReplay, undo: S.modeDesign, gone: S.replayControls },
  /*
   * The delete preflight is deliberately absent, and this is the one entry that
   * needs saying so.
   *
   * Its Cancel was pressed by nothing for a while, which is a real gap — but it
   * cannot be closed here. A return edge holds the child's render to the
   * *parent's* expectations, and the parent of an open preflight is a card whose
   * expectations include its fixture's own copy. Opening the preflight answers
   * through `runCrudIntent`, which calls `refreshState` and republishes the
   * host's config over the injected payload, so by the time Cancel has closed
   * the prompt the page is no longer showing the fixture the parent was
   * enumerated with. Declared as reversible, the edge fails on `missing-copy`
   * for a reason that is about the harness rather than about the control.
   *
   * That is the same fact `a-preflight-answers-with-the-hosts-own-config`
   * already names. So the undo is walked by `probe--delete-refusal-dismissed`
   * instead, which presses the same Cancel and carries expectations that do not
   * depend on a fixture the preflight has already replaced.
   */
  // The fold on a finished concurrent group, and the one toggle whose first
  // press *removes* a region. It sits on the card rather than inside the member
  // list precisely so that collapsing does not take the only button that could
  // undo it, and that claim is only under test if the matrix walks it back.
  { via: S.groupFold, undo: S.groupFold, hidden: S.groupMembers },
];

/**
 * The transition DAG. An edge exists when the operations on one state's path
 * turn it into another reachable state — which is exactly what the browser
 * suite executes, so every drawn edge is one the suite has walked.
 *
 * Each edge carries the `delta`: the operations to apply to a page already
 * sitting in `from` to arrive at `to`. The suite walks that, rather than
 * re-entering the child from scratch, so the edge is the thing under test.
 *
 * Two kinds. An `enter` edge is a prefix relation: the child's path is the
 * parent's plus the delta. A `return` edge is the way back out, and it is
 * not derivable from any path, because no state's *entry* contains it.
 */
export const TRANSITIONS = buildTransitions();

/** The prefix subset: the part that is a DAG and is asserted to stay one. */
export const ENTRY_TRANSITIONS = TRANSITIONS.filter((edge) => edge.kind === "enter");

function buildTransitions() {
  const byPath = new Map(STATES.map((state) => [signature(state.ops), state.id]));
  const edges = [];
  for (const state of STATES) {
    const acting = state.ops.filter(isAction);
    const found = nearestParent(byPath, acting, state.id);
    if (found) {
      const delta = deltaAfter(state.ops, found.actions);
      edges.push({ kind: "enter", from: found.id, to: state.id, via: describe(delta), delta });
    }
  }
  return [...edges, ...edges.map(returnEdge).filter(Boolean)];
}

/**
 * The nearest ancestor: the longest proper action-prefix that is itself a state.
 *
 * It used to be the prefix exactly one action short, which quietly meant that
 * any step a user takes in *two* operations was not a transition at all. That
 * lost the whole create-and-rename family — `select a kind` then `press Add` is
 * one act of creating a step, and typing a name then pressing Enter is one act
 * of renaming it — so the editor's most-used controls were entered by every
 * state that needed them and walked as an edge by none.
 *
 * Longest-first is what keeps the parent honest: a state is attributed to the
 * closest screen it can actually be reached from, rather than to some distant
 * ancestor that happens to share an opening. For a kind the sample has no step
 * of, that closest screen is the created step itself — a decision step is
 * reached by making one — and the edge says so.
 */
function nearestParent(byPath, acting, id) {
  for (let actions = acting.length - 1; actions >= 1; actions -= 1) {
    const found = byPath.get(JSON.stringify(acting.slice(0, actions)));
    if (found && found !== id) {
      return { id: found, actions };
    }
  }
  return null;
}

/** The way back out of `edge`, when the control it used declares an undo. */
function returnEdge(edge) {
  const last = edge.delta.filter(isAction).at(-1);
  const toggle = last?.op === "click" && REVERSIBLE.find((item) => item.via === last.selector);
  if (!toggle) {
    return null;
  }
  // A toggle that revealed a region is undone when the region has gone; one
  // that removed a region is undone when it is back. Waiting the wrong way
  // round would be an edge that passes on the instant it is called.
  const settled = toggle.gone
    ? { op: "waitGone", selector: toggle.gone }
    : { op: "wait", selector: toggle.hidden };
  return {
    kind: "return",
    from: edge.to,
    to: edge.from,
    via: `click ${toggle.undo}`,
    delta: [{ op: "click", selector: toggle.undo }, settled],
  };
}

/**
 * Everything the child's path does after the parent's last action.
 *
 * Not "from the child's own new action": the waits in between belong to the
 * step being taken, not to the ground the parent already covered. `run:
 * running` waits for its run to be listed before selecting it, and a delta that
 * began at the select left that wait in the parent's half of the path — where
 * the parent has never done it, because the parent has no run to wait for.
 */
function deltaAfter(ops, actions) {
  let seen = 0;
  const last = ops.findIndex((op) => isAction(op) && ++seen === actions);
  return ops.slice(last + 1);
}

function signature(ops) {
  return JSON.stringify(ops.filter(isAction));
}

/**
 * The label: every acting operation the delta performs, in order.
 *
 * The whole delta rather than its last operation, because a delta can now
 * carry more than one — and an edge labelled only "press Enter" would be
 * naming the smaller half of renaming a step. `test/statelab.test.mjs` rebuilds
 * this from the two states' own paths and compares, so a label that drifts from
 * the work it claims fails there.
 */
function describe(delta) {
  return delta.filter(isAction).map(describeOp).join(" → ");
}

function describeOp(op) {
  if (op.op === "click") {
    return `click ${op.selector}`;
  }
  // `press` belongs here rather than in the fall-through it used to take: an
  // edge labelled a bare "press" named neither the control nor the key, so
  // "fill the name → press" described renaming a step without saying what
  // committed it, or to what.
  if (op.op === "fill" || op.op === "select" || op.op === "press") {
    return `${op.op} ${op.selector} = ${JSON.stringify(op.value)}`;
  }
  if (op.op === "drag") {
    return `drag ${op.selector} by ${op.dx},${op.dy}`;
  }
  if (op.op === "fixture") {
    return `publish ${[].concat(op.value).join(" + ")}`;
  }
  return op.op;
}

/**
 * Why nothing reaches a state first.
 *
 * `EXCLUSIONS` records why a combination is not a state. Its counterpart was
 * missing. A state that nothing arrives at is a claim about the product too,
 * and the graph made every one of them the same claim — the bare words "a root
 * of the DAG", which are true of the screen the canvas opens on and equally
 * true of a tuple whose parent quietly failed to be a state. A reviewer could
 * not tell those apart, and the second is the first's defect wearing its
 * clothes.
 *
 * The count was worse than unattributed. It was carried in prose and asserted
 * nowhere, so it drifted: the number claimed was 69 while the graph held 136.
 * Deriving it here is what makes it capable of being wrong.
 *
 * Roots are taken over `ENTRY_TRANSITIONS`, not every edge. A return edge is
 * the way back out of a screen this one opens, so arriving along it means the
 * reader was already here — it is not a first arrival, and counting it as one
 * made a landing's status depend on whether some child of it happened to
 * appear in `REVERSIBLE`. Eleven states were hidden from the count that way:
 * eight landings, the config and editor screens the canvas actually opens on,
 * and three probes. `test/statelab.test.mjs` names that set and requires every
 * one of them to be a root, so this paragraph cannot drift the way the count
 * above it once did.
 *
 * The categories are ordered and the first match wins, so each state is named
 * by the strongest fact about it: a saving field is intercepted *and* has a
 * fixture its parent lacks, and "cannot be walked into" is the reason that
 * matters. Two boundaries carry real weight, on disjoint sets of three probes:
 * three probes also satisfy `landing`, and three ride a request route, so
 * moving `landing` up or `probe` above `intercepted` relabels one set or the
 * other with a reason that is false of it. `test/statelab.test.mjs` pins the
 * resulting tally — either reordering fails there by name.
 */
export const ROOT_REASONS = [
  {
    id: "boot",
    title: "a condition on the load itself",
    why: "The whole content of this state is what happened to the page load — a payload held open, or a renderer module refused. There is no earlier screen for an edge to leave from, because the surface has not been drawn yet.",
    holds: (state) => state.surface === "boot" || state.surface === "boot-editor",
  },
  {
    id: "intercepted",
    title: "reached by a route, not by a click",
    why: "This state rides on a request interception installed before the page loads. A page already sitting on the parent screen cannot acquire that route, so an edge into it would name a transition the suite could not walk.",
    holds: (state) => Boolean(state.intercept),
  },
  {
    id: "probe",
    title: "a hand-assembled crossing",
    why: "A probe is written to cross two dimensions a scoping rule keeps apart, rather than enumerated from them, so its path is nobody's extension by construction.",
    holds: (state) => state.kind === "probe",
  },
  {
    id: "landing",
    title: "where the reader arrives",
    why: "Loading the page and publishing its fixture are the entire path. Nothing precedes it: this is the screen the canvas opens on for that config.",
    holds: (state) => state.ops.filter(isAction).every((op) => op.op === "page" || op.op === "fixture"),
  },
  {
    id: "fixture-differs",
    title: "the screen above it publishes a different fixture",
    why: "The clicks that reach this state are a real screen's clicks, but the fixture it needs is chosen by its deepest axis and the screen above it is enumerated with a different one. So the prefix names the route a reader really takes and still matches no state, and the suite walks the whole path from the load rather than claiming an edge it could not follow.",
    holds: () => true,
  },
];

/** The reason nothing reaches a state first. Ordered; the first match wins. */
export function rootReason(state) {
  return ROOT_REASONS.find((reason) => reason.holds(state));
}

const FIRST_ARRIVAL = new Set(ENTRY_TRANSITIONS.map((edge) => edge.to));

/**
 * Every state no entry edge reaches, each carrying the reason it has none.
 * The counterpart to `EXCLUSIONS`, and reported the same way.
 */
export const ROOTS = STATES.filter((state) => !FIRST_ARRIVAL.has(state.id)).map((state) => ({
  id: state.id,
  reason: rootReason(state).id,
}));

/** What the PR body and the lab header both report. */
export function summary() {
  return {
    dimensions: DIMENSIONS.length,
    dimensionValues: DIMENSIONS.reduce((total, item) => total + item.values.length, 0),
    combinations: walked.combinations,
    constraintRules: CONSTRAINTS.length,
    structuralRules: CONSTRAINTS.filter((rule) => rule.kind === "structural").length,
    scopingRules: CONSTRAINTS.filter((rule) => rule.kind === "scoping").length,
    harnessRules: CONSTRAINTS.filter((rule) => rule.kind === "harness").length,
    excludedCombinations: EXCLUSIONS.reduce((total, entry) => total + entry.pruned, 0),
    matrixStates: matrixStates.length,
    probes: PROBES.length,
    states: STATES.length,
    transitions: TRANSITIONS.length,
    entryTransitions: ENTRY_TRANSITIONS.length,
    returnTransitions: TRANSITIONS.length - ENTRY_TRANSITIONS.length,
    roots: ROOTS.length,
  };
}

export { CONSTRAINTS, DIMENSIONS, ORDER };
