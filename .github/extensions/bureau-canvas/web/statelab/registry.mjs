// The state matrix: the Cartesian product of every dimension value, filtered
// by the named rules in `constraints.mjs`, with an entry path and an
// expectation set attached to each survivor.
//
// Pure and deterministic. The lab, the offline tests and the browser suite all
// read this module; none of them keeps its own list of states, so a state that
// is not here is not rendered, screenshotted or asserted anywhere.

import { CONSTRAINTS } from "./constraints.mjs";
import { DIMENSIONS, valueOf, valuesOf } from "./dimensions.mjs";
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
 * interception, which the browser suite does with `page.route` and the lab
 * cannot do from inside an iframe — so these states carry `intercept`, and the
 * lab blanks its stage and names the reason rather than leaving the previous
 * state's render on screen. The suite is where they are rendered and captured.
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
  if (combo.run === "refused") {
    return [...(valueOf("mode", combo.mode)?.enter ?? []), ...runRefusalOps()];
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
];

/**
 * The transition DAG. An edge exists when one operation on a state's path
 * turns it into another reachable state — which is exactly what the browser
 * suite executes, so every drawn edge is one the suite has walked.
 *
 * Each edge carries the `delta`: the operations to apply to a page already
 * sitting in `from` to arrive at `to`. The suite walks that, rather than
 * re-entering the child from scratch, so the edge is the thing under test.
 *
 * Two kinds. An `enter` edge is a prefix relation: the child's path is the
 * parent's plus one operation. A `return` edge is the way back out, and it is
 * not derivable from any path, because no state's *entry* contains it.
 */
export const TRANSITIONS = buildTransitions();

/** The prefix subset: the part that is a DAG and is asserted to stay one. */
export const ENTRY_TRANSITIONS = TRANSITIONS.filter((edge) => edge.kind === "enter");

function buildTransitions() {
  const byPath = new Map(STATES.map((state) => [signature(state.ops), state.id]));
  const edges = [];
  for (const state of STATES) {
    const acting = state.ops.filter((op) => op.op !== "wait");
    if (acting.length < 2) {
      continue;
    }
    const parent = byPath.get(JSON.stringify(acting.slice(0, -1)));
    if (parent && parent !== state.id) {
      edges.push({ kind: "enter", from: parent, to: state.id, via: describe(acting.at(-1)), delta: deltaFrom(state.ops, acting.length - 1) });
    }
  }
  return [...edges, ...edges.map(returnEdge).filter(Boolean)];
}

/** The way back out of `edge`, when the control it used declares an undo. */
function returnEdge(edge) {
  const last = edge.delta.filter((op) => op.op !== "wait").at(-1);
  const toggle = last?.op === "click" && REVERSIBLE.find((item) => item.via === last.selector);
  if (!toggle) {
    return null;
  }
  return {
    kind: "return",
    from: edge.to,
    to: edge.from,
    via: `click ${toggle.undo}`,
    delta: [{ op: "click", selector: toggle.undo }, { op: "waitGone", selector: toggle.gone }],
  };
}

/** The tail of `ops` beginning at the (skip + 1)-th acting operation. */
function deltaFrom(ops, skip) {
  let seen = 0;
  const start = ops.findIndex((op) => op.op !== "wait" && seen++ === skip);
  return ops.slice(start);
}

function signature(ops) {
  return JSON.stringify(ops.filter((op) => op.op !== "wait"));
}

function describe(op) {
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
