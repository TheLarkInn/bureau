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
import { draftOps, EDIT_PATHS, FIELD_LIFECYCLE, fixtureFor, interceptFor, runOps, runRefusalOps, selectStep } from "./paths.mjs";
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
  const bag = { shows: new Set(), hides: new Set(), copy: new Set(), allowErrors: new Set(), allowPlaceholder: new Set() };
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
  };
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
  // The draft bar sits above the body on both index surfaces, so its own save
  // is walked last — after the body has settled into whatever rest it is in.
  return [...ops, ...bySurface[combo.surface](), ...draftOps(combo.draft)];
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
  if (op.op === "fill" || op.op === "select") {
    return `${op.op} ${op.selector} = ${JSON.stringify(op.value)}`;
  }
  if (op.op === "fixture") {
    return `publish ${[].concat(op.value).join(" + ")}`;
  }
  return op.op;
}

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
  };
}

export { CONSTRAINTS, DIMENSIONS, ORDER };
