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
import { EDIT_PATHS, FIELD_LIFECYCLE, fixtureFor, runOps, selectStep } from "./paths.mjs";
import { PROBES } from "./probes.mjs";
import { SELECTORS as S } from "./selectors.mjs";

const ORDER = ["surface", "data", "draft", "section", "disclosure", "card", "field", "fieldState", "mode", "run", "tab", "pick", "edit"];

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
  const shows = new Set();
  const hides = new Set();
  const copy = new Set();
  const allowErrors = new Set();
  const suppressed = new Set(ORDER.flatMap((key) => valueOf(key, combo[key])?.suppress ?? []));
  for (const key of ORDER) {
    const value = valueOf(key, combo[key]);
    if (suppressed.has(key) || (value?.only && !value.only.includes(combo.surface))) {
      continue;
    }
    for (const source of [value, value?.derive?.(combo)]) {
      for (const item of source?.shows ?? []) {
        shows.add(item);
      }
      for (const item of source?.hides ?? []) {
        hides.add(item);
      }
      for (const item of source?.copy ?? []) {
        copy.add(item);
      }
      for (const item of source?.allowErrors ?? []) {
        allowErrors.add(item);
      }
    }
  }
  for (const item of FIELD_LIFECYCLE[combo.field]?.[combo.fieldState]?.copy ?? []) {
    copy.add(item);
  }
  return {
    shows: [...shows].filter((item) => !hides.has(item)),
    hides: [...hides],
    copy: [...copy],
    allowErrors: [...allowErrors],
  };
}

/**
 * The pre-surface states are the only ones a click cannot reach: one needs the
 * renderer module blocked, the other needs `/state` held open. Both are request
 * interception, which the browser suite does with `page.route` and the lab
 * cannot do from inside an iframe — so these states carry `intercept` and the
 * lab shows the suite's captured render with that reason attached.
 *
 * Each page boots itself, so each has both: index.html swaps in its dedicated
 * fallback shell, editor.html replaces its root with a plain status line.
 */
function bootOps(combo) {
  const page = pageFor(combo);
  if (combo.data !== "render-error") {
    return [{ op: "page", value: page, intercept: "stall-state" }, { op: "wait", selector: S.loading }];
  }
  const intercept = page === "editor" ? "block-editor-renderer" : "block-renderer";
  return [{ op: "page", value: page, intercept }, { op: "wait", selector: page === "editor" ? S.loading : S.fallback }];
}

function configOps(combo) {
  const ops = [];
  for (const key of ["section", "disclosure", "card", "field"]) {
    ops.push(...(valueOf(key, combo[key])?.enter ?? []));
  }
  ops.push(...(FIELD_LIFECYCLE[combo.field]?.[combo.fieldState]?.ops ?? []));
  return ops;
}

function pipelineOps(combo) {
  return [...(valueOf("mode", combo.mode)?.enter ?? []), ...runOps(combo.mode, combo.run)];
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

/** The full entry path: load a page, publish a fixture, then act like a user. */
function entryPath(combo) {
  if (combo.surface === "boot" || combo.surface === "boot-editor") {
    return bootOps(combo);
  }
  const ops = [{ op: "page", value: pageFor(combo) }, { op: "fixture", value: fixtureFor(combo) }];
  const bySurface = {
    config: () => [{ op: "wait", selector: S.configView }, ...configOps(combo)],
    pipeline: () => [{ op: "wait", selector: S.pipelineView }, ...pipelineOps(combo)],
    editor: () => [{ op: "wait", selector: S.editorTabs }, ...editorOps(combo)],
  };
  return [...ops, ...bySurface[combo.surface]()];
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

/** Per-rule exclusion accounting: how many tuples each rule removed, and one example. */
export const EXCLUSIONS = walked.excluded.map((entry) => ({
  ...entry,
  ...pick(CONSTRAINTS.find((rule) => rule.id === entry.rule), ["kind", "title", "why"]),
}));

function pick(source, keys) {
  return Object.fromEntries(keys.map((key) => [key, source?.[key]]));
}

/** Reachable states: the matrix plus the deliberate crossing probes. */
export const STATES = [...matrixStates, ...PROBES];

/**
 * The transition DAG. An edge exists when one operation on a state's path
 * turns it into another reachable state — which is exactly what the browser
 * suite executes, so every drawn edge is one the suite has walked.
 *
 * Each edge carries the `delta`: the operations to apply to a page already
 * sitting in `from` to arrive at `to`. The suite walks that, rather than
 * re-entering the child from scratch, so the edge is the thing under test.
 */
export const TRANSITIONS = buildTransitions();

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
      edges.push({ from: parent, to: state.id, via: describe(acting.at(-1)), delta: deltaFrom(state.ops, acting.length - 1) });
    }
  }
  return edges;
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
    excludedCombinations: EXCLUSIONS.reduce((total, entry) => total + entry.count, 0),
    matrixStates: matrixStates.length,
    probes: PROBES.length,
    states: STATES.length,
    transitions: TRANSITIONS.length,
  };
}

export { CONSTRAINTS, DIMENSIONS, ORDER };
