// The state registry, checked without a browser.
//
// The registry decides what the lab renders and what the browser suite
// asserts, so its own invariants have to hold before either runs: exact
// exclusion accounting, distinguishable states, entry paths that only use
// verbs the driver knows, and fixture data that stays offline and matches the
// payload the host actually serves.
//
// Offline by construction: this file reads one committed JSON fixture and
// imports pure modules. No network, no browser, no `bureau` binary.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { CONSTRAINT_IDS, CONSTRAINTS, violations } from "../web/statelab/constraints.mjs";
import { DIMENSIONS, valuesOf } from "../web/statelab/dimensions.mjs";
import { CONTRAST, verdict } from "../web/statelab/checks.mjs";
import { ADAPTER_VERBS } from "../web/statelab/driver.mjs";
import { enumerate } from "../web/statelab/enumerate.mjs";
import { applyFixture, FIXTURE_IDS, FIXTURES } from "../web/statelab/fixtures.mjs";
import { SAMPLE_STEP_COUNT, RUN_END, RUN_IDS } from "../web/statelab/paths.mjs";
import { EXCLUSIONS, ORDER, STATES, summary, TRANSITIONS } from "../web/statelab/registry.mjs";

const PAYLOAD = new URL("./fixtures/committed-payload.json", import.meta.url);

async function servedState() {
  const payload = JSON.parse(await readFile(PAYLOAD, "utf8"));
  const config = payload.config;
  const assignments = Object.values(config.assignments).map((item) => ({
    name: item.name,
    work: {
      forge: item.work.forge,
      source: item.work.source,
      filter: item.work.filter,
      approvalLabel: item.work.approval_label,
      abortLabel: item.work.abort_label,
      escalateLabel: item.work.escalate_label,
    },
    repos: item.repos,
    pipeline: item.pipeline,
    branchPrefix: item.branch_prefix,
    limits: { maxConcurrent: item.limits.max_concurrent, maxRunsPerHour: item.limits.max_runs_per_hour },
  }));
  return {
    dir: payload.dir,
    status: "Showing bundled sample; config directory not found.",
    validation: { ok: true, state: "fixture", dir: payload.dir, errors: [], message: "Showing bundled sample" },
    findings: [],
    findingsByItem: {},
    findingsByStep: {},
    generalFindings: [],
    plan: null,
    selectedPipeline: null,
    pipelines: Object.fromEntries(Object.keys(config.pipelines).map((name) => [name, { view: { name, steps: [], edges: [], terminals: [] } }])),
    config: {
      view: {
        dir: payload.dir,
        repos: Object.entries(config.repos).map(([name, repo]) => ({ name, ...repo })),
        roles: Object.values(config.roles),
        assignments,
        pipelines: Object.keys(config.pipelines).map((name) => ({ name })),
        orphans: [],
      },
      relation: { nodes: [], edges: [] },
    },
  };
}

test("every excluded combination is attributed, and the books balance", () => {
  const counts = summary();
  const attributed = EXCLUSIONS.reduce((total, entry) => total + entry.pruned, 0);

  assert.deepStrictEqual(
    {
      balances: attributed + counts.matrixStates === counts.combinations,
      everyRuleAccountedFor: EXCLUSIONS.length === CONSTRAINTS.length,
      everyRemovalHasAnExample: EXCLUSIONS.every((entry) => entry.pruned === 0 || entry.example),
      noRuleIsDeadWeight: EXCLUSIONS.every((entry) => entry.pruned > 0),
    },
    { balances: true, everyRuleAccountedFor: true, everyRemovalHasAnExample: true, noRuleIsDeadWeight: true },
  );
});

/**
 * The balance above is an identity of the traversal — it cannot fail. The
 * property it stands in for can: the walk prunes a rule the moment every
 * dimension in its `reads` is assigned, so a rule whose `holds()` consults a
 * dimension it did not declare fires while that dimension is still undefined
 * and cuts a subtree it had no right to. Every other assertion here would
 * still pass, because the wrongly-cut tuples are still counted and
 * `violations()` only re-checks survivors.
 *
 * A read trap catches it directly: run each rule against tuples that report
 * every key access, and require that nothing outside `reads` is touched. The
 * tuples must cover every value of every dimension, because a rule can consult
 * an undeclared dimension on only one branch — an earlier sampler here stepped
 * by a fixed stride and so pinned every seven-valued dimension to a single
 * value for all 200 draws, leaving exactly that mutation invisible.
 */
test("no rule reads a dimension it did not declare", () => {
  const trespass = new Set();
  const sampleValues = (dimension) => valuesOf(dimension).map((value) => value.id);
  const rotations = Math.max(...ORDER.map((dimension) => sampleValues(dimension).length));
  for (const rule of CONSTRAINTS) {
    for (const pivot of ORDER) {
      for (const pivotValue of sampleValues(pivot)) {
        for (let rotation = 0; rotation < rotations; rotation += 1) {
          rule.holds(watch(rule, trespass, tupleAround(pivot, pivotValue, rotation, sampleValues)));
        }
      }
    }
  }
  assert.deepStrictEqual([...trespass], []);
});

/** Every dimension varies with `rotation`; `pivot` is pinned to one value. */
function tupleAround(pivot, pivotValue, rotation, sampleValues) {
  return Object.fromEntries(ORDER.map((dimension, index) => {
    const values = sampleValues(dimension);
    const value = dimension === pivot ? pivotValue : values[(rotation + index) % values.length];
    return [dimension, value];
  }));
}

function watch(rule, trespass, tuple) {
  return new Proxy(tuple, {
    get(target, key) {
      if (typeof key === "string" && ORDER.includes(key) && !rule.reads.includes(key)) {
        trespass.add(`${rule.id} reads ${key}`);
      }
      return target[key];
    },
  });
}

/**
 * And the claim that follows from it: the surviving set is a property of the
 * rules, not of `ORDER`. Re-enumerating under permuted orders must keep
 * exactly the same tuples. (The per-rule `pruned` tallies legitimately move,
 * which is why they are named for the walk rather than for the rule.)
 */
test("the kept set does not depend on the order dimensions are assigned in", () => {
  const canonical = new Set(STATES
    .filter((state) => state.kind === "matrix")
    .map((state) => ORDER.map((key) => state.dimensions[key]).join("|")));
  const mismatches = [];
  for (let seed = 1; seed <= 4; seed += 1) {
    const permuted = [...ORDER].sort((left, right) => hash(left + seed) - hash(right + seed));
    const kept = new Set(enumerate(permuted, valuesOf).kept.map((combo) => ORDER.map((key) => combo[key]).join("|")));
    if (kept.size !== canonical.size || [...kept].some((tuple) => !canonical.has(tuple))) {
      mismatches.push(permuted.join(","));
    }
  }
  assert.deepStrictEqual(mismatches, []);
});

function hash(value) {
  return [...value].reduce((total, character) => (total * 31 + character.charCodeAt(0)) % 9973, 7);
}

test("every kept combination survives a fresh run of every rule", () => {
  const offenders = STATES.filter((state) => state.kind === "matrix" && violations(state.dimensions).length);
  assert.deepStrictEqual(offenders.map((state) => state.id), []);
});

test("states are distinguishable: no two share an id or an entry path", () => {
  const ids = STATES.map((state) => state.id);
  const paths = STATES.map((state) => JSON.stringify(state.ops));
  assert.deepStrictEqual(
    { duplicateIds: ids.length - new Set(ids).size, duplicatePaths: paths.length - new Set(paths).size },
    { duplicateIds: 0, duplicatePaths: 0 },
  );
});

/**
 * Unique entry paths are necessary but not sufficient: two paths that name
 * different fixtures still render the same screen if those fixtures project to
 * the same payload. A fixture that quietly stopped changing anything — an
 * edited `twoAssignments()` that returns its input, say — would leave every
 * path unique and every check passing while two "different" states were one.
 * So the layers are compared as data: each must move the payload, and no two
 * may land on the same one.
 */
test("every fixture changes the payload, and no two fixtures agree", async () => {
  const base = await servedState();
  const projected = new Map();
  for (const id of FIXTURE_IDS) {
    projected.set(id, JSON.stringify(applyFixture([id], base)));
  }
  // `sample` is the one deliberate identity: it publishes the host's own
  // payload, which is what lets every non-boot path publish something and so
  // makes the ordering assertion above meaningful. Every other layer must move.
  const inert = FIXTURE_IDS.filter((id) => id !== "sample" && projected.get(id) === JSON.stringify(base));
  const collisions = FIXTURE_IDS
    .filter((id, index) => FIXTURE_IDS.some((other, position) => position < index && projected.get(other) === projected.get(id)));

  assert.deepStrictEqual(
    { inert, collisions, sampleIsIdentity: projected.get("sample") === JSON.stringify(base) },
    { inert: [], collisions: [], sampleIsIdentity: true },
  );
});

test("every entry path uses only verbs the driver implements", () => {
  const verbs = new Set(["page", "fixture", ...ADAPTER_VERBS.filter((verb) => !["goto", "publish"].includes(verb))]);
  const unknown = STATES.flatMap((state) => state.ops.filter((op) => !verbs.has(op.op)).map((op) => `${state.id}: ${op.op}`));
  assert.deepStrictEqual(unknown, []);
});

/**
 * "Publishes before it acts" has to require a publish. Comparing indices alone
 * did not: a path with no `fixture` op scored -1, which is less than every
 * action index, so a state that never published its own payload — and so
 * rendered whatever the host happened to serve — passed as ordered. Boot is
 * the one honest exception: it is the surface shown *before* a payload exists.
 */
test("every entry path loads a page first and publishes exactly once before it acts", () => {
  const broken = STATES.filter((state) => {
    const acting = state.ops.filter((op) => !["wait", "waitGone", "present"].includes(op.op));
    const publishes = acting.filter((op) => op.op === "fixture").length;
    const firstAction = acting.findIndex((op) => !["page", "fixture"].includes(op.op));
    if (acting[0]?.op !== "page") {
      return true;
    }
    if (BOOT_SURFACES.includes(state.dimensions?.surface)) {
      return publishes !== 0;
    }
    return publishes !== 1 || acting[1]?.op !== "fixture" || (firstAction !== -1 && firstAction < 2);
  });
  assert.deepStrictEqual(broken.map((state) => state.id), []);
});

const BOOT_SURFACES = ["boot", "boot-editor"];

test("every state names fixtures that exist, and every fixture is used", () => {
  const named = new Set(STATES.flatMap((state) => [].concat(state.fixture ?? [])));
  assert.deepStrictEqual(
    {
      unknown: [...named].filter((id) => !FIXTURE_IDS.includes(id)),
      unused: FIXTURE_IDS.filter((id) => !named.has(id)),
    },
    { unknown: [], unused: [] },
  );
});

test("fixtures compose over the served payload without mutating it", async () => {
  const base = await servedState();
  const before = JSON.stringify(base);
  const composed = applyFixture(["invalid", "multi-repo", "draft-pending", "pipeline"], base);

  assert.deepStrictEqual(
    {
      untouched: JSON.stringify(base) === before,
      status: composed.status,
      repos: composed.config.view.assignments[0].repos,
      pending: composed.plan.writes.length + composed.plan.removals.length,
      selected: composed.selectedPipeline.name,
    },
    {
      untouched: true,
      status: "Validation findings",
      repos: ["bureau", "bureau-docs"],
      pending: 3,
      selected: "agent-eligible-pipeline",
    },
  );
});

test("every fixture is a pure projection of the served payload", async () => {
  const base = await servedState();
  const results = FIXTURE_IDS.map((id) => {
    const once = JSON.stringify(applyFixture(id, base));
    return { id, stable: once === JSON.stringify(applyFixture(id, base)), object: once.startsWith("{") };
  });
  assert.deepStrictEqual(results.filter((row) => !row.stable || !row.object), []);
});

test("the sample pipeline still has the step count the registry addresses", async () => {
  const payload = JSON.parse(await readFile(PAYLOAD, "utf8"));
  assert.equal(payload.config.pipelines["agent-eligible-pipeline"].steps.length, SAMPLE_STEP_COUNT);
});

test("the transition DAG only names states the registry holds", () => {
  const ids = new Set(STATES.map((state) => state.id));
  const dangling = TRANSITIONS.filter((edge) => !ids.has(edge.from) || !ids.has(edge.to));
  assert.deepStrictEqual({ dangling: dangling.length, acyclic: !hasCycle(TRANSITIONS) }, { dangling: 0, acyclic: true });
});

function hasCycle(edges) {
  const next = new Map();
  for (const edge of edges) {
    next.set(edge.from, [...(next.get(edge.from) ?? []), edge.to]);
  }
  const seen = new Set();
  const walk = (node, stack) => {
    if (stack.has(node)) {
      return true;
    }
    if (seen.has(node)) {
      return false;
    }
    seen.add(node);
    return (next.get(node) ?? []).some((child) => walk(child, new Set([...stack, node])));
  };
  return [...next.keys()].some((node) => walk(node, new Set()));
}

test("every scoping rule is held to account by a crossing probe that really breaks it", () => {
  const probes = STATES.filter((state) => state.kind === "probe");
  const crossings = probes.filter((state) => state.rule);
  const probed = new Set(crossings.map((state) => state.rule));
  assert.deepStrictEqual(
    {
      unchecked: CONSTRAINTS.filter((rule) => rule.kind === "scoping" && !probed.has(rule.id)).map((rule) => rule.id),
      dangling: crossings.filter((state) => !CONSTRAINT_IDS.includes(state.rule)).map((state) => `${state.id} -> ${state.rule}`),
      // The claim under test: a crossing's tuple must actually be rejected by
      // the rule it names. Naming a rule it does not break is the defect this
      // replaces — a label that reads as evidence and is not.
      unbroken: crossings
        .filter((state) => !violations(state.dimensions).includes(state.rule))
        .map((state) => `${state.id} -> ${state.rule}`),
      // A probe is one or the other, never both and never neither.
      unlabelled: probes.filter((state) => Boolean(state.rule) === Boolean(state.covers)).map((state) => state.id),
    },
    { unchecked: [], dangling: [], unbroken: [], unlabelled: [] },
  );
});

/**
 * The old form compared `parent.ops + delta` against `child.ops` with waits
 * stripped, which is how `buildTransitions` constructs the delta — an identity,
 * not a test. What the DAG actually claims is that a page already sitting in
 * the parent can reach the child by applying only the delta. That holds when
 * everything the child's path does *before* the delta has already been done by
 * the parent's path, waits included.
 *
 * Parent matching ignores waits, so a child that interleaves a wait its parent
 * never performs would silently lose that wait from the delta and race. No
 * edge does that today; this is what stops one being added quietly. Parents
 * may wait for *more* than the child's prefix — a settled page is never the
 * problem — so the check is subsequence containment, not equality.
 */
test("every edge's delta is preceded by work the parent has already done", () => {
  const byId = new Map(STATES.map((state) => [state.id, state]));
  const broken = TRANSITIONS.filter((edge) => {
    const child = byId.get(edge.to).ops;
    const prefix = child.slice(0, child.length - edge.delta.length);
    return !isSubsequence(prefix, byId.get(edge.from).ops);
  });
  assert.deepStrictEqual(broken.map((edge) => `${edge.from} -> ${edge.to}`), []);
});

/** Does every op of `needles` appear in `haystack`, in order? */
function isSubsequence(needles, haystack) {
  let cursor = 0;
  for (const op of needles) {
    const found = haystack.findIndex((item, index) => index >= cursor && JSON.stringify(item) === JSON.stringify(op));
    if (found === -1) {
      return false;
    }
    cursor = found + 1;
  }
  return true;
}

/**
 * The old form asserted three identities of `deltaFrom`: that the delta holds
 * one acting op, that it is shorter than the child's path, and that it equals
 * the tail of the child's path it was sliced from. All three are true by
 * construction, so no registry edit could turn it red.
 *
 * What the DAG actually claims is that the *edge label* names the work the
 * delta does, and that applying it to the parent lands on the child. So the
 * delta is rebuilt here from the two states' own paths — parent's acting ops
 * removed from the child's — without consulting `edge.delta`, and the labels
 * are derived independently too. A `buildTransitions` that mislabelled an
 * edge, or that shipped a delta which skipped or repeated an operation, now
 * fails.
 */
test("every edge's delta is the child's path minus the parent's, and says so", () => {
  const byId = new Map(STATES.map((state) => [state.id, state]));
  const broken = TRANSITIONS.filter((edge) => {
    const parentActing = byId.get(edge.from).ops.filter((op) => op.op !== "wait").length;
    const rebuilt = tailAfter(byId.get(edge.to).ops, parentActing);
    return JSON.stringify(rebuilt) !== JSON.stringify(edge.delta) || label(rebuilt) !== edge.via;
  });
  assert.deepStrictEqual(broken.map((edge) => `${edge.from} -> ${edge.to}`), []);
});

/** The child's ops from its (skip + 1)-th acting operation onwards. */
function tailAfter(ops, skip) {
  let seen = 0;
  const start = ops.findIndex((op) => op.op !== "wait" && seen++ === skip);
  return start === -1 ? [] : ops.slice(start);
}

/** The edge label, rebuilt from the delta rather than read off the edge. */
function label(delta) {
  const acting = delta.filter((op) => op.op !== "wait");
  return acting.map((op) => describeOp(op)).join(" → ");
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

/**
 * The registry addresses the replay timeline by the span of the run it is
 * showing, which is the one thing that tells three replayed runs apart. That
 * span is the last `at_ms` in a committed log, so it is read back from the log
 * here: a fixture edited without the registry would otherwise leave the matrix
 * asserting a `max` that no run produces.
 */
test("every run span the registry addresses is the end of that run's log", async () => {
  const ends = {};
  for (const [value, runId] of Object.entries(RUN_IDS)) {
    const log = await readFile(new URL(`./fixtures/runs/${runId}/events.jsonl`, import.meta.url), "utf8");
    const events = log.trim().split("\n").map((line) => JSON.parse(line));
    ends[value] = events.at(-1).at_ms;
  }
  assert.deepStrictEqual(ends, RUN_END);
});

test("every dimension and rule carries the prose the lab shows", () => {
  const thin = [
    ...DIMENSIONS.filter((item) => !item.title || !item.why || !item.values.every((value) => value.summary)).map((item) => item.id),
    ...CONSTRAINTS.filter((rule) => !rule.title || !rule.why || !rule.reads?.length).map((rule) => rule.id),
    ...Object.values(FIXTURES).filter((fixture) => !fixture.summary || !fixture.layer).map((fixture) => fixture.id),
  ];
  assert.deepStrictEqual(thin, []);
});

test("every rule reads dimensions that exist", () => {
  const known = new Set(ORDER);
  const unknown = CONSTRAINTS.flatMap((rule) => rule.reads.filter((name) => !known.has(name)).map((name) => `${rule.id}: ${name}`));
  assert.deepStrictEqual(unknown, []);
});

test("the verdict reports missing controls, missing copy, low contrast, overlap and clipping", () => {
  const state = { expect: { shows: [".present", ".absent"], hides: [".leaked"], copy: ["expected copy"] } };
  const snapshot = {
    counts: { ".present": 1, ".absent": 0, ".leaked": 2 },
    text: "something else",
    viewport: { width: 800, height: 600 },
    overflowX: 40,
    contrast: [{ selector: ".kind-label", text: "AGENT", ratio: 1.07 }],
    boxes: [
      { selector: ".assignment-card", x: 0, y: 0, width: 100, height: 100 },
      { selector: ".assignment-card", x: 50, y: 50, width: 100, height: 100 },
      { selector: ".draft-bar", x: 700, y: 0, width: 300, height: 40 },
    ],
  };
  assert.deepStrictEqual(verdict(state, snapshot).map((item) => item.kind).sort(), [
    "clipped",
    "horizontal-overflow",
    "low-contrast",
    "missing-control",
    "missing-copy",
    "overlap",
    "unexpected-control",
  ]);
});

test("the verdict catches one landing region printing over another", () => {
  const state = { expect: { shows: [], hides: [], copy: [] } };
  const snapshot = {
    counts: {},
    text: "",
    viewport: { width: 1280, height: 900 },
    overflowX: 0,
    contrast: [],
    // One card, one draft bar, drawn on top of it. Same-selector comparison
    // sees nothing here: there is one box of each kind.
    boxes: [
      { selector: ".assignment-card", x: 0, y: 60, width: 600, height: 200 },
      { selector: ".draft-bar", x: 0, y: 100, width: 600, height: 48 },
    ],
  };
  assert.deepStrictEqual(verdict(state, snapshot), [
    { kind: "overlap", detail: ".draft-bar overlaps .assignment-card" },
  ]);
});

test("a render that matches the registry produces no findings", () => {
  const state = { expect: { shows: [".present"], hides: [".leaked"], copy: ["Work Source"] } };
  const snapshot = {
    counts: { ".present": 1, ".leaked": 0 },
    text: "WORK SOURCE — link a board",
    viewport: { width: 1280, height: 900 },
    overflowX: 0,
    contrast: [{ selector: ".kind-label", text: "AGENT", ratio: 5.2 }],
    boxes: [{ selector: ".assignment-card", x: 0, y: 0, width: 100, height: 100 }],
  };
  assert.deepStrictEqual(verdict(state, snapshot), []);
});

test("every contrast selector names small text coloured from a kind hue", () => {
  // A hue that reads as decoration rather than text is a defect the
  // screenshots would not show, so the ratio is asserted rather than eyeballed.
  assert.ok(CONTRAST.includes(".kind-label"));
  assert.deepStrictEqual(CONTRAST.filter((selector) => !selector.startsWith(".")), []);
});
