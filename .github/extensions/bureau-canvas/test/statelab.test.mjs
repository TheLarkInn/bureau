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

import { CONSTRAINT_IDS, CONSTRAINTS, harnessNotes, violations } from "../web/statelab/constraints.mjs";
import { CONCURRENT_STATE } from "../web/statelab/concurrent-state.mjs";
import { buildConcurrentState, PROJECTED_FIELDS } from "./support/concurrent-state.mjs";
import { relationView } from "../lib/view.mjs";
import { DIMENSIONS, valueOf, valuesOf } from "../web/statelab/dimensions.mjs";
import { collect, CONTRAST, deadlineVerdict, graphsDrawn, measureFor, permitted, selectorsFor, SETTLE_BUDGET_MS, settleStep, undrawnFor, undrawnGraphs, undrawnLooks, unsettledReason, verdict } from "../web/statelab/checks.mjs";
import { ADAPTER_VERBS, isAction } from "../web/statelab/driver.mjs";
import { enumerate } from "../web/statelab/enumerate.mjs";
import { applyFixture, FIXTURE_IDS, FIXTURES } from "../web/statelab/fixtures.mjs";
import { SAMPLE_STEP_COUNT, RUN_END, RUN_IDS, RUN_STEP, interceptFor } from "../web/statelab/paths.mjs";
import { EXCLUSIONS, ENTRY_TRANSITIONS, ORDER, RENDER_TWINS, REVERSIBLE, rootReason, ROOT_REASONS, ROOTS, STATES, summary, TRANSITIONS } from "../web/statelab/registry.mjs";
import { VIEWPORTS, SELECTORS } from "../web/statelab/selectors.mjs";
import { emptyVerdict, PANEL_CLEAN, PANEL_ELSEWHERE, PANEL_UNCHECKED } from "../web/panel-verdict.mjs";

const PAYLOAD = new URL("./fixtures/committed-payload.json", import.meta.url);
const CONCURRENT_PAYLOAD = new URL("./fixtures/concurrent-payload.json", import.meta.url);

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
      // The real projector, so the base the fixtures compose over is the graph
      // `buildState` would actually serve rather than a stub. A hand-written
      // one would make the consistency check below an identity of this file.
      relation: relationView(payload),
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
 * Every `enter` a dimension value declares has to reach a path.
 *
 * `entryPath` consumes `enter` from a hand-written list of axes, and `surface`
 * was not on it. So the pipeline surface's `wait` for the Live badge to settle
 * — declared with a comment stating it happened "once, on the surface all three
 * modes are entered from" — was dropped from every state, and `liveCountSettled`
 * had exactly one reference in the repository: its own dead declaration. The
 * `data-count` assertions were left racing an in-flight fetch, saved only by the
 * suite's re-sample loop.
 *
 * A missing consumer is invisible to every other test here, because a dropped
 * `enter` produces a path that is merely shorter — still valid, still walkable,
 * still passing. This is the one assertion that can see it, and it is written
 * over the whole vocabulary rather than over `surface` alone, so the next axis
 * to grow an `enter` is covered on the day it is added rather than after.
 */
test("no dimension value declares an entry operation that no state performs", () => {
  const performed = new Set(STATES.flatMap((state) => state.ops).map((op) => JSON.stringify(op)));
  const orphaned = DIMENSIONS
    .flatMap((dimension) => dimension.values.map((value) => ({ axis: dimension.id, value })))
    .flatMap(({ axis, value }) => (value.enter ?? []).map((op) => ({ axis, value: value.id, op })))
    .filter((entry) => !performed.has(JSON.stringify(entry.op)));

  assert.deepStrictEqual(orphaned, []);
});

/**
 * Every number this branch reports about itself, pinned to a literal.
 *
 * `summary()` is what the PR body and the lab header both read, and only two of
 * its fields were ever compared with anything: the root split, and the internal
 * identity above — which, as its own comment says, cannot fail. The rest were
 * derived, printed and believed. That is precisely the defect this registry
 * exists to refuse, and it had already produced a wrong claim: a root count
 * carried in prose that drifted to 69 while the graph held 136, with nothing
 * able to say so.
 *
 * Returning `dimensionValues + 1` passed every other test in this file.
 *
 * Pinning the whole object is deliberately brittle. A new dimension value or
 * state is *meant* to stop here and be looked at, rather than have the branch
 * quietly restate its own new arithmetic as though it were the reviewed one.
 *
 * `renders` is not a field of `summary()` — it is what the gallery and the
 * matrix suite actually produce — so it is derived from the only two things
 * that decide it and pinned alongside, because it is reported the same way.
 */
test("every count the branch reports about itself is what the registry holds", () => {
  const renders = STATES.length * Object.keys(VIEWPORTS).length;

  assert.deepStrictEqual(
    { ...summary(), renders },
    {
      dimensions: 16,
      dimensionValues: 98,
      combinations: 705438720000,
      constraintRules: 33,
      structuralRules: 22,
      scopingRules: 7,
      harnessRules: 4,
      excludedCombinations: 705438719779,
      matrixStates: 221,
      probes: 49,
      states: 270,
      transitions: 137,
      entryTransitions: 101,
      returnTransitions: 36,
      roots: 169,
      renders: 540,
    },
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
  const walks = [];
  for (let seed = 1; seed <= 4; seed += 1) {
    const permuted = shuffled(ORDER, seed);
    walks.push(permuted.join(","));
    const kept = new Set(enumerate(permuted, valuesOf).kept.map((combo) => ORDER.map((key) => combo[key]).join("|")));
    if (kept.size !== canonical.size || [...kept].some((tuple) => !canonical.has(tuple))) {
      mismatches.push(permuted.join(","));
    }
  }
  // Four seeds are four samples only if they are four different walks, and the
  // first version of this test was not: it sorted by `hash(key + seed)`, where
  // the seed is folded in last and so shifts every key by the same constant.
  // The relative order never moved, and all four iterations re-ran one
  // permutation. The count is asserted so that regressing the shuffle fails
  // here rather than quietly costing three quarters of the coverage.
  assert.deepStrictEqual(
    [mismatches, new Set(walks).size, walks.filter((walk) => walk === ORDER.join(",")).length],
    [[], 4, 0],
  );
});

/**
 * A seeded Fisher-Yates. Every dimension can land in every position, so a
 * different seed is a genuinely different walk rather than the same one
 * relabelled.
 */
function shuffled(keys, seed) {
  const order = [...keys];
  let state = (seed * 2654435761) % 2147483647;
  for (let index = order.length - 1; index > 0; index -= 1) {
    state = (state * 1103515245 + 12345) % 2147483648;
    const pick = state % (index + 1);
    [order[index], order[pick]] = [order[pick], order[index]];
  }
  return order;
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

/**
 * A twin declares that two states draw one screen on purpose. Declared against
 * an id no state has, it suppresses nothing and asserts nothing — an inert
 * sentence that reads like a decision — so the ids are held to the registry and
 * the reason is required to be a sentence rather than a placeholder.
 */
/**
 * A twin declares that two states draw one screen on purpose. Declared against
 * an id no state has, it suppresses nothing and asserts nothing — an inert
 * sentence that reads like a decision — so the ids are held to the registry and
 * the reason is required to be a sentence rather than a placeholder.
 *
 * An empty `viewports` is the same inert sentence one level down, and it read
 * as clean: `keysFor` maps over the list, so a twin naming no viewport
 * generates no pair, suppresses no `undeclared-twin` finding and produces no
 * `unchecked-twin` either. The declaration would sit in the file looking like a
 * reviewed decision while the audit had never been told about it.
 */
test("every declared render twin names two real states, at real viewports, with a reason", () => {
  const ids = new Set(STATES.map((state) => state.id));
  const viewports = new Set(Object.values(VIEWPORTS).map((viewport) => viewport.id));
  const faults = RENDER_TWINS.flatMap((twin) => [
    ...[twin.a, twin.b].filter((id) => !ids.has(id)).map((id) => `no state ${id}`),
    ...((twin.viewports ?? []).length ? [] : [`${twin.a} declares no viewport`]),
    ...(twin.viewports ?? []).filter((id) => !viewports.has(id)).map((id) => `no viewport ${id}`),
    ...(twin.a === twin.b ? [`${twin.a} is declared a twin of itself`] : []),
    ...((twin.why ?? "").length > 20 ? [] : [`${twin.a} declares no reason`]),
  ]);

  assert.deepStrictEqual(faults, []);
});

test("every entry path uses only verbs the driver implements", () => {  const verbs = new Set(["page", "fixture", ...ADAPTER_VERBS.filter((verb) => !["goto", "publish"].includes(verb))]);
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

test("every fixture ships the relation projection its own config implies", async () => {
  const base = await servedState();
  // Role edges are the one class a *view* cannot yield, so they are taken from
  // the projections of the raw configs the fixtures are built from: the served
  // sample, and the committed payload `concurrent-run` carries. Deriving them
  // from the base alone was sound only while no fixture brought its own
  // pipeline, and that premise is now false by design rather than by accident —
  // so the sources are named here, and `newPipelines` still fails a fixture
  // that invents a pipeline neither of them explains.
  const known = relationView(JSON.parse(await readFile(CONCURRENT_PAYLOAD, "utf8")));
  const drawn = {
    edges: [...base.config.relation.edges, ...known.edges],
    nodes: [...base.config.relation.nodes, ...known.nodes],
  };
  assert.deepStrictEqual(FIXTURE_IDS.flatMap((id) => disagreements(id, applyFixture(id, base), drawn)), []);
});

/**
 * `relationView` derives the graph from the config's own lists: one node pe
 * assignment, pipeline, role and repo, one edge per pipeline or repo an
 * assignment names, and one per role a pipeline's steps name — keeping
 * only edges whose endpoints are both nodes, which is why an assignment
 * pointing at an unregistered repo owes none.
 *
 * A fixture that adds to one list and not the other therefore builds a payload
 * `buildState` could never serve: a header counting two repos above a graph
 * drawing one, or a card the graph has no node for. The state reviewing that is
 * reviewing a screen no user can reach. This has been the same defect three
 * times — `orphans`, `two-assignments`, `multi-repo` — so it is a gate now
 * rather than a comment on each fixture.
 *
 * The comparison is a set equality in both directions, not a containment. An
 * earlier form asked only "is every owed edge drawn?", which let a fixture add
 * an edge between two existing nodes, or drop a pipeline's role edge, and stay
 * green — the graph would then assert a relation the config does not state,
 * which is the same class of lie as a missing one.
 *
 * Role edges are the one class the view cannot yield, because `view.pipelines`
 * carries names and `usedBy` and never steps. They are taken from the raw
 * configs the fixtures are built from instead, which is sound only while every
 * pipeline a fixture draws comes from one of them; that premise is checked here
 * rather than assumed.
 */
function disagreements(id, state, base) {
  const view = state.config?.view ?? {};
  const relation = state.config?.relation ?? { nodes: [], edges: [] };
  const drawn = new Set(relation.nodes.map((node) => node.id));
  const listed = itemIds(view);
  const roleEdges = base.edges.filter((edge) => edge.relation === "role");
  const owed = [...(view.assignments ?? []).flatMap(assignmentEdges), ...roleEdges]
    .filter((edge) => drawn.has(edge.source) && drawn.has(edge.target));
  const owedKeys = new Set(owed.map(edgeKey));
  const wired = new Set(relation.edges.map(edgeKey));
  return [
    ...listed.filter((item) => !drawn.has(item)).map((item) => `${id}: config lists ${item}, the graph has no node for it`),
    ...[...drawn].filter((node) => !listed.includes(node)).map((node) => `${id}: the graph draws ${node}, the config does not list it`),
    ...[...owedKeys].filter((edge) => !wired.has(edge)).map((edge) => `${id}: both ends are drawn but the edge ${edge} is missing`),
    ...[...wired].filter((edge) => !owedKeys.has(edge)).map((edge) => `${id}: the graph draws the edge ${edge}, the config does not imply it`),
    ...relation.edges.filter((edge) => edge.id !== edgeKey(edge)).map((edge) => `${id}: the edge id ${edge.id} disagrees with its own ends`),
    ...newPipelines(relation.nodes, base.nodes).map((node) => `${id}: introduces ${node}, whose role edges this gate cannot derive`),
  ];
}

/** Pipeline nodes a fixture drew that the base projection did not. */
function newPipelines(nodes, baseNodes) {
  const known = new Set(baseNodes.filter((node) => node.kind === "pipeline").map((node) => node.id));
  return nodes.filter((node) => node.kind === "pipeline" && !known.has(node.id)).map((node) => node.id);
}

function edgeKey(edge) {
  return `${edge.relation}:${edge.source}->${edge.target}`;
}

function itemIds(view) {
  return ["assignment", "pipeline", "role", "repo"].flatMap((kind) => (view[`${kind}s`] ?? []).map((item) => `${kind}:${item.name}`));
}

function assignmentEdges(item) {
  const source = `assignment:${item.name}`;
  return [
    { relation: "pipeline", source, target: `pipeline:${item.pipeline}` },
    ...(item.repos ?? []).map((repo) => ({ relation: "repo", source, target: `repo:${repo}` })),
  ];
}

test("the sample pipeline still has the step count the registry addresses", async () => {
  const payload = JSON.parse(await readFile(PAYLOAD, "utf8"));
  assert.equal(payload.config.pipelines["agent-eligible-pipeline"].steps.length, SAMPLE_STEP_COUNT);
});

test("the transition DAG only names states the registry holds", () => {
  const ids = new Set(STATES.map((state) => state.id));
  const dangling = TRANSITIONS.filter((edge) => !ids.has(edge.from) || !ids.has(edge.to));
  // Acyclicity is a property of the *entry* relation only. A return edge is a
  // cycle by definition — that is what "the way back" means — so asserting it
  // over every edge would forbid the half of the graph a user spends most of
  // their time in. The prefix subset is what has to stay a DAG.
  assert.deepStrictEqual(
    { dangling: dangling.length, acyclic: !hasCycle(ENTRY_TRANSITIONS) },
    { dangling: 0, acyclic: true },
  );
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

/**
 * `EXCLUSIONS` says why a combination is not a state. `ROOTS` says why nothing
 * reaches one first — the same question asked of the graph rather than the
 * product, and it went unasked for a while. The number it would have produced
 * was carried in the PR body instead, where it was wrong and nothing could say
 * so.
 *
 * The tally is pinned per category rather than merely totalled, because the
 * categories overlap and their order is what resolves the overlap. Two
 * boundaries do real work, on disjoint sets of three probes each. Three probes
 * also satisfy `landing`, so listing `landing` first would relabel them "the
 * screen the canvas opens on for that config", which is false of a hand-written
 * crossing. Three others ride a request route, so listing `probe` before
 * `intercepted` would relabel *those* "a hand-assembled crossing written to
 * cross two dimensions a scoping rule keeps apart" — false of `probe--create-saving`,
 * which crosses no rule and is unreachable because of the route. A total alone
 * survives either reordering unchanged — the three move between categories and
 * the sum does not notice. Pinning the split is what makes the documented
 * ordering load-bearing.
 *
 * The `intercepted`/`probe` boundary only became load-bearing once probes
 * carried their route on the state as well as on the op that installs it.
 * Before that the `intercepted` predicate could not fire for a probe at all,
 * and swapping the two categories left this tally byte-identical.
 *
 * Pinned numbers go stale by design: a new state that lands in a category has
 * to be looked at, which is the review this registry exists to force.
 *
 * `RETURN_ONLY_ROOTS` is the other half, and it is the one that pins the
 * *definition* rather than the arithmetic. These are the states a return edge
 * arrives at and no entry edge does — eight landings and three probes. Under
 * the all-edges definition this change replaced, every one of them was
 * silently not a root. Asserting they are roots fails that revert by name;
 * asserting merely that no root is entered does not, because the all-edges
 * roots are a subset of these and so satisfy it too.
 */
const ROOT_TALLY = { boot: 4, intercepted: 101, probe: 20, landing: 36, "fixture-differs": 8 };
const RETURN_ONLY_ROOTS = 11;

test("every state nothing reaches first is attributed, and the books balance", () => {
  const entered = new Set(ENTRY_TRANSITIONS.map((edge) => edge.to));
  const roots = STATES.filter((state) => !entered.has(state.id));
  const rootIds = new Set(ROOTS.map((root) => root.id));
  const returnOnly = roots.filter((state) => TRANSITIONS.some((edge) => edge.to === state.id));
  const tally = Object.fromEntries(
    ROOT_REASONS.map((reason) => [reason.id, ROOTS.filter((root) => root.reason === reason.id).length]),
  );

  assert.deepStrictEqual(
    {
      tally,
      countsAgree: ROOTS.length === roots.length && ROOTS.length === summary().roots,
      returnOnly: returnOnly.length,
      returnOnlyAreRoots: returnOnly.every((state) => rootIds.has(state.id)),
      everyCategoryExplainsItself: ROOT_REASONS.every((reason) => Boolean(reason.title && reason.why)),
    },
    {
      tally: ROOT_TALLY,
      countsAgree: true,
      returnOnly: RETURN_ONLY_ROOTS,
      returnOnlyAreRoots: true,
      everyCategoryExplainsItself: true,
    },
  );
});

/**
 * The catch-all makes a specific claim — that the clicks reaching these states
 * are some real screen's clicks, and only the published fixture differs — and
 * a catch-all is where an untrue claim hides, because it absorbs whatever the
 * named categories did not.
 *
 * So it is checked rather than trusted. Blinding the fixture's value alone is
 * not enough to discriminate: 17 of the probe roots satisfy that too, and
 * would be absorbed with a reason that is false of them, since a probe's
 * fixture is hand-written rather than chosen by an axis. `kind === "matrix"`
 * is the gate that does that work.
 *
 * The fixture comparison is kept for what it documents rather than what it
 * decides: for a root it cannot change an outcome, because `blind` erases only
 * the fixture op's *value*, so a blinded match with an equal fixture would be
 * an exact match — which `nearestParent` would have found, giving the state an
 * entry edge and disqualifying it as a root. It records the half of the claim
 * that the state's own rootness already guarantees.
 */
test("the catch-all root category names a cause that really holds", () => {
  const blind = (ops) => JSON.stringify(ops.map((op) => (op.op === "fixture" ? { op: "fixture" } : op)));
  const byBlindPath = new Map(STATES.map((state) => [blind(state.ops.filter(isAction)), state]));
  const fixtureOf = (state) => JSON.stringify([].concat(state.fixture ?? []));
  const explains = (state) => {
    const acting = state.ops.filter(isAction);
    return acting.slice(1).some((_, index) => {
      const ancestor = byBlindPath.get(blind(acting.slice(0, acting.length - 1 - index)));
      return Boolean(ancestor) && fixtureOf(ancestor) !== fixtureOf(state);
    });
  };
  const caught = ROOTS.map((root) => STATES.find((state) => state.id === root.id))
    .filter((state) => rootReason(state).id === "fixture-differs");
  const unexplained = caught.filter((state) => state.kind !== "matrix" || !explains(state));

  // `caught` is asserted non-empty by name rather than against itself: a
  // category that has stopped catching anything would make the second half
  // vacuously true, which is the failure this whole test exists to refuse.
  assert.deepStrictEqual(
    { catchesSomething: caught.length > 0, unexplained: unexplained.map((state) => state.id) },
    { catchesSomething: true, unexplained: [] },
  );
});

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
      // `violations` applies every rule, assigned inputs or not, so a tuple
      // missing a dimension picks up unrelated rules it never meant to break:
      // an absent `transport` is not `"n/a"`, so it reads as a transport on a
      // run that is not being replayed. The crossing then breaks its own rule
      // *and* a structural one, and `unbroken` still passes because it only
      // asks whether the named rule is in the list. A crossing has to be a
      // point in the product to stand for one, so it owes every axis a value
      // the axis actually declares.
      incomplete: crossings.flatMap((state) => malformed(state)),
      // A probe is one or the other, never both and never neither.
      unlabelled: probes.filter((state) => Boolean(state.rule) === Boolean(state.covers)).map((state) => state.id),
    },
    { unchecked: [], dangling: [], unbroken: [], incomplete: [], unlabelled: [] },
  );
});

/**
 * A `harness` rule is the one kind that excludes a screen a user really
 * reaches, so it is the one kind that can quietly cost coverage. Four claims
 * are checked. The third is the one that cannot be talked around: enumerate
 * again without the rule, and if the kept set does not grow then the rule hides
 * nothing of its own and is claiming a cost it does not impose.
 *
 * The fourth closes the hole the other three left. `stands` was asked for
 * nothing but *existence* — any state id in the registry satisfied it — so the
 * field that is supposed to say "here is the excluded screen, reached from
 * somewhere this harness can get to" could name a state with no relation to the
 * exclusion at all, and the check would still read green. A name that reads as
 * evidence and is not is the exact defect `unbroken` was written to remove from
 * crossings, left standing on the rule kind where the cost is highest.
 *
 * So the standing state is held to two properties instead. It must satisfy the
 * rule — a state the rule itself excludes is not somewhere the harness can
 * reach — and it must sit *adjacent* to the excluded region: changing exactly
 * one of the axes the rule reads must produce a combination the rule rejects.
 * That is the strongest thing that is actually true here, and it is worth being
 * exact about why it is not "renders the same screen". These screens differ by
 * one axis on purpose, because the axis is what the harness cannot reach: the
 * clean selection of a decision step stands on a *created* one, which carries a
 * dirty bar the excluded screen would not have. Claiming the two renders match
 * would be the same overclaim in the other direction. What `stands` promises,
 * and what this now holds it to, is that a reviewer looking at it is one named
 * axis away from the screen the rule removed, rather than somewhere else
 * entirely.
 */
test("every harness rule names its limit, stands next to the screen it hides, and really hides one", () => {
  const harness = CONSTRAINTS.filter((rule) => rule.kind === "harness");
  const rendered = new Set(STATES.map((state) => state.id));
  const base = enumerate(ORDER, valuesOf).kept.length;
  const withoutRule = (rule) => enumerate(ORDER, valuesOf, CONSTRAINTS.filter((item) => item.id !== rule.id)).kept.length;

  assert.deepStrictEqual(
    {
      unnamed: harness.filter((rule) => !rule.limit?.trim()).map((rule) => rule.id),
      unstood: harness.filter((rule) => !rendered.has(rule.stands)).map((rule) => rule.id),
      // Reported only for a rule whose standing state exists, so a bad name is
      // one finding rather than two.
      unadjacent: harness.filter((rule) => rendered.has(rule.stands) && !standsNextTo(rule)).map((rule) => rule.id),
      costless: harness.filter((rule) => withoutRule(rule) <= base).map((rule) => rule.id),
      // The obligations belong to the kind. A structural or scoping rule
      // carrying them reads as a harness limit that was never re-kinded.
      mislabelled: CONSTRAINTS.filter((rule) => rule.kind !== "harness" && (rule.limit || rule.stands)).map((rule) => rule.id),
      unknownKind: CONSTRAINTS.filter((rule) => !["structural", "scoping", "harness"].includes(rule.kind)).map((rule) => rule.id),
    },
    { unnamed: [], unstood: [], unadjacent: [], costless: [], mislabelled: [], unknownKind: [] },
  );
});

/**
 * And that the lab tells the reviewer the claim above, not a stronger one.
 *
 * The test above is careful that `stands` promises adjacency and not sameness —
 * "these screens differ by one axis on purpose, because the axis is what the
 * harness cannot reach". The lab said the opposite, in both places it printed a
 * harness rule: *"The same screen is rendered by …"*. The suite held the weaker,
 * true claim while the review surface handed a reviewer the stronger, false one,
 * which is the worst place for it: someone told two screens match has been given
 * a reason not to look at the difference, and looking is the whole job.
 *
 * Held here rather than in the browser spec because the sentence is data now:
 * one function, read by both call sites, so neither can drift and the offline
 * suite can ask what it says without a lab to render it.
 */
test("the lab's harness note names the limit and the nearest state, and claims no more", () => {
  const harness = CONSTRAINTS.filter((rule) => rule.kind === "harness");
  const said = harness.map((rule) => ({ rule: rule.id, notes: harnessNotes(rule).join(" ") }));

  assert.deepStrictEqual(
    {
      silent: said.filter(({ notes }) => !notes.includes("Harness limit —")).map(({ rule }) => rule),
      unattributed: said.filter(({ rule, notes }) => !notes.includes(CONSTRAINTS.find((item) => item.id === rule).stands)).map(({ rule }) => rule),
      // The overclaim itself, named rather than described, because a sentence
      // that asserts the two renders match is the one thing nothing here holds.
      overclaiming: said.filter(({ notes }) => /the same screen is rendered/iu.test(notes)).map(({ rule }) => rule),
      unqualified: said.filter(({ notes }) => !notes.includes("not the same screen")).map(({ rule }) => rule),
    },
    { silent: [], unattributed: [], overclaiming: [], unqualified: [] },
  );
});

/**
 * Whether a rule's standing state is reachable past that rule and one axis away
 * from a combination *this rule alone* rejects.
 *
 * Asked against the rule's own predicate rather than against an enumerated
 * example, because `enumerate` keeps one worked example per rule and which
 * tuple that is depends on `ORDER`. A check that compared `stands` against that
 * example would pass or fail on the walk order rather than on the registry,
 * which is a mark rather than a check by a different route.
 *
 * The neighbour must also satisfy every *other* constraint, and that half was
 * missing. Adjacency asked only `!rule.holds(neighbour)`, so a state qualified
 * by sitting next to a combination that some unrelated rule already excluded —
 * a combination that is not a screen this harness rule hides, and in one
 * measured case the only neighbour a `stands` had. The lab then told a reviewer
 * "the nearest state this harness can reach" about a state adjacent to nothing
 * the harness was keeping from them. A witness that is itself not a screen
 * proves nothing about the screen being stood in for.
 */
function standsNextTo(rule) {
  const stands = STATES.find((state) => state.id === rule.stands);
  if (!stands?.dimensions || !rule.holds(stands.dimensions)) {
    return false;
  }
  const others = CONSTRAINTS.filter((item) => item.id !== rule.id);
  return rule.reads.some((axis) =>
    valuesOf(axis).some((value) => {
      const neighbour = { ...stands.dimensions, [axis]: value.id };
      return !rule.holds(neighbour) && others.every((item) => item.holds(neighbour));
    }));
}

/**
 * The blind spot the check above leaves, closed from the other side.
 *
 * `costless` only ever asks rules that are already kinded `harness`, and
 * `mislabelled` only catches the reverse mistake — so a harness limit wearing a
 * `structural` label is asked for nothing at all, which is how two of them sat
 * here unnoticed. There is no general test for "does this screen exist in the
 * product?", but there is an exact one for the way it goes wrong: a rule whose
 * verdict is computed from the *fixture's* step inventory is a statement about
 * this bundle, not about the product. The screens it removes are ordinary ones
 * that a user with a richer pipeline reaches, which is the definition of a
 * harness limit.
 *
 * `none` is asserted alongside so the check keeps its teeth: if `SAMPLE_STEPS`
 * is ever renamed or inlined, this test starts matching nothing and would
 * otherwise pass by vacuity for the rest of its life.
 */
test("a rule that decides from the fixture's step inventory is kinded as the harness limit it is", () => {
  const readsFixture = CONSTRAINTS.filter((rule) => rule.holds.toString().includes("SAMPLE_STEPS"));

  assert.deepStrictEqual(
    {
      none: readsFixture.length === 0,
      mislabelled: readsFixture.filter((rule) => rule.kind !== "harness").map((rule) => rule.id),
    },
    { none: false, mislabelled: [] },
  );
});

/** Axes a crossing left unset, or set to a value its axis does not declare. */
function malformed(state) {
  return ORDER.flatMap((key) => {
    const value = state.dimensions[key];
    if (value === undefined) {
      return [`${state.id}: no value for ${key}`];
    }
    const declared = valuesOf(key).map((item) => item.id);
    return declared.includes(value) ? [] : [`${state.id}: ${key}=${value} is not a value of ${key}`];
  });
}

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
  const broken = ENTRY_TRANSITIONS.filter((edge) => {
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
  const broken = ENTRY_TRANSITIONS.filter((edge) => {
    const parentActing = byId.get(edge.from).ops.filter(isAction).length;
    const rebuilt = tailAfter(byId.get(edge.to).ops, parentActing);
    return JSON.stringify(rebuilt) !== JSON.stringify(edge.delta) || label(rebuilt) !== edge.via;
  });
  assert.deepStrictEqual(broken.map((edge) => `${edge.from} -> ${edge.to}`), []);
});

/**
 * A return edge makes a different claim from an entry edge, so it gets its own
 * gate. It says: the page is sitting in `from`, one click on a real control
 * puts it back in `to`, and `to` is the state it originally came from.
 *
 * The last part is what stops a plausible-looking undo from being wired to the
 * wrong screen. A return edge is only meaningful as the mirror of an entry
 * edge, so every one of them has to have a partner pointing the other way, and
 * its delta has to be a single click plus the wait that proves the region it
 * moved really moved. The browser suite then executes it and holds the rende
 * to `to`'s own expectations, which is where "opens but never closes" — o
 * "closes and will not open" — actually fails.
 *
 * Two shapes, because a toggle goes both ways: the undo of a control that
 * revealed a region waits for it to go, and the undo of one that removed a
 * region waits for it to come back. Both are one click and one wait; a delta
 * that is anything else is not an undo.
 */
test("every return edge mirrors an entry edge and undoes exactly one control", () => {
  const entries = new Set(ENTRY_TRANSITIONS.map((edge) => `${edge.from} -> ${edge.to}`));
  const returns = TRANSITIONS.filter((edge) => edge.kind === "return");
  const shapeOf = (edge) => edge.delta.map((op) => op.op).join("+");
  const shapes = ["click+waitGone", "click+wait"];
  assert.deepStrictEqual(
    {
      unmirrored: returns.filter((edge) => !entries.has(`${edge.to} -> ${edge.from}`)).map((edge) => `${edge.from} -> ${edge.to}`),
      misshapen: returns.filter((edge) => !shapes.includes(shapeOf(edge))).map((edge) => `${edge.from} -> ${edge.to}: ${shapeOf(edge)}`),
      unlabelled: returns.filter((edge) => edge.via !== `click ${edge.delta[0].selector}`).map((edge) => `${edge.from} -> ${edge.to}`),
      none: returns.length === 0,
      // Both shapes must actually occur, or the pair above is one live branch
      // and one that has never been taken.
      restoring: returns.some((edge) => shapeOf(edge) === "click+wait"),
    },
    { unmirrored: [], misshapen: [], unlabelled: [], none: false, restoring: true },
  );
});

/**
 * The other direction of the same claim, and the one the test above cannot
 * make: that every control declared reversible actually has an edge.
 *
 * `returnEdge` only fires for a control that ends an entry edge, and an entry
 * edge is a prefix relation over the whole op list — the fixture included. So a
 * declared undo whose state publishes a payload its parent does not simply
 * produces nothing, silently, and the suite walks eleven toggles minus howeve
 * many were quietly dead. The forge-signals disclosure was exactly that: named
 * in `REVERSIBLE`, asserted nowhere, on the very control this work changed from
 * open-only to a toggle.
 *
 * A toggle is matched by the control that *opened* it rather than by its undo,
 * because two toggles may share one undo — Live and Replay both leave by the
 * Design button. Keyed on the undo, one mode-design return edge answered fo
 * both, and the whole of Replay could have gone dead behind Live's edge.
 */
test("every reversible control declares an undo the suite actually walks", () => {
  const opened = new Set(ENTRY_TRANSITIONS.map((edge) => `${edge.from}→${edge.to}:${edge.via}`));
  const walked = TRANSITIONS.filter((edge) => edge.kind === "return")
    .map((edge) => `${edge.to}→${edge.from}:`);
  const covered = (toggle) => walked.some((back) => opened.has(`${back}click ${toggle.via}`));
  const dead = REVERSIBLE.filter((toggle) => !covered(toggle)).map((toggle) => toggle.via);
  assert.deepStrictEqual({ dead, declared: REVERSIBLE.length === 0 }, { dead: [], declared: false });
});

/**
 * And that the region a return edge waits on was ever in the state it claims.
 *
 * A return edge is `click undo` then a wait, and either wait passes instantly
 * on a selector that never matches. So a region that names nothing downgrades
 * the edge from "this control moved something" to "this control was clicked",
 * silently — the same shape of nothing as a declared toggle with no edge, one
 * level down.
 *
 * Requiring the two states to promise the region is what makes the wait mean
 * something. A `gone` toggle reveals it, so the state it opens has to show it;
 * a `hidden` toggle takes it away, so the state it opens has to say it is
 * absent *and* the state it left has to have had it — otherwise "it came back"
 * is a claim about a region neither screen ever mentions.
 */
test("every reversible control names a region both of its states account for", () => {
  const byId = new Map(STATES.map((state) => [state.id, state]));
  const unpromised = ENTRY_TRANSITIONS.flatMap((edge) => {
    const toggle = REVERSIBLE.find((item) => edge.via === `click ${item.via}`);
    if (!toggle) {
      return [];
    }
    const child = byId.get(edge.to)?.expect ?? { shows: [], hides: [] };
    const parent = byId.get(edge.from)?.expect ?? { shows: [], hides: [] };
    if (toggle.gone) {
      return child.shows.includes(toggle.gone) ? [] : [`${edge.to} does not show ${toggle.gone}`];
    }
    return [
      ...(child.hides.includes(toggle.hidden) ? [] : [`${edge.to} does not name ${toggle.hidden} absent`]),
      ...(parent.shows.includes(toggle.hidden) ? [] : [`${edge.from} does not show ${toggle.hidden}`]),
    ];
  });
  assert.deepStrictEqual(unpromised, []);
});

/** A toggle declares exactly one region, and says which way it moves it. */
test("every reversible control says which way it moves its region", () => {
  const malformed = REVERSIBLE
    .filter((toggle) => Boolean(toggle.gone) === Boolean(toggle.hidden))
    .map((toggle) => toggle.via);
  assert.deepStrictEqual({ malformed, declared: REVERSIBLE.length === 0 }, { malformed: [], declared: false });
});

/** The child's ops from its (skip + 1)-th acting operation onwards. */
function tailAfter(ops, actions) {
  let seen = 0;
  const last = ops.findIndex((op) => isAction(op) && ++seen === actions);
  return last === -1 ? [] : ops.slice(last + 1);
}

/** The edge label, rebuilt from the delta rather than read off the edge. */
function label(delta) {
  const acting = delta.filter(isAction);
  return acting.map((op) => describeOp(op)).join(" → ");
}

function describeOp(op) {
  if (op.op === "click") {
    return `click ${op.selector}`;
  }
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
 * The label check above rebuilds the text with a copy of the registry's own
 * formatter, so on the *format* half the two agree by construction: an op kind
 * the formatter drops is dropped identically on both sides and nothing fails.
 * That is not hypothetical — `press` fell through to a bare "press" for exactly
 * this reason, and the copy here fell through with it, so "fill the name →
 * press" passed while naming neither the key nor the control it went to.
 *
 * So this holds labels to the property the formatter exists to have, rather
 * than to a second copy of the formatter: whatever an operation acts on, and
 * whatever value it uses, the label says so. A new op kind added to the
 * vocabulary and not to `describeOp` fails here on the day it is used, which is
 * the case the duplicated version can never reach.
 */
test("every edge label names the selector each operation acts on and the value it uses", () => {
  const lossy = TRANSITIONS.flatMap((edge) => edge.delta
    .filter(isAction)
    .filter((op) => [op.selector, ...[].concat(op.value ?? [])].some((part) => part && !edge.via.includes(part)))
    .map((op) => `${edge.from} -> ${edge.to}: ${op.op} in ${JSON.stringify(edge.via)}`));
  assert.deepStrictEqual(lossy, []);
});

/**
 * The registry addresses the replay timeline by the span of the run it is
 * showing, which is the one thing that tells three replayed runs apart. That
 * span is the last `at_ms` in a committed log, so it is read back from the log
 * here: a fixture edited without the registry would otherwise leave the matrix
 * asserting a `max` that no run produces.
 *
 * A span belongs to a *replayed* run, and not every run value is one — `ended`
 * is the live chrome after its transport was withdrawn and never reaches a
 * timeline. So the tables are held to exactly the run values the registry
 * actually replays, read off the states themselves: a value that gains a replay
 * state without a span fails here, and so does a span for a run nothing replays.
 */
test("every run span the registry addresses is the end of that run's log", async () => {
  const replayed = new Set(STATES
    .filter((state) => state.dimensions?.mode === "replay" && state.dimensions.run !== "none")
    .map((state) => state.dimensions.run));
  const ends = {};
  const steps = {};
  for (const value of replayed) {
    const log = await readFile(new URL(`./fixtures/runs/${RUN_IDS[value]}/events.jsonl`, import.meta.url), "utf8");
    const stamps = log.trim().split("\n").map((line) => JSON.parse(line).at_ms);
    ends[value] = stamps.at(-1);
    // The transport's two claims, derived the way `stepBy` derives them: park
    // at the first event, and one forward step lands on the next distinct
    // stamp. Without this the axis addressed a position no log had to produce.
    const next = Math.min(...stamps.filter((at) => at > stamps[0]));
    steps[value] = { start: stamps[0], next, readout: `+${((next - stamps[0]) / 1000).toFixed(1)}s` };
  }
  assert.deepStrictEqual({ ends, steps }, { ends: RUN_END, steps: RUN_STEP });
});

/**
 * Every run value the registry names has a committed log behind it, replayed
 * or not. The span test above only reaches the replayed ones, so without this a
 * live-only value could address a run directory that does not exist and nothing
 * offline would say so — the browser suite would just fail to find an option.
 */
test("every run value the registry picks names a committed log", async () => {
  const missing = [];
  for (const [value, runId] of Object.entries(RUN_IDS)) {
    const log = await readFile(new URL(`./fixtures/runs/${runId}/events.jsonl`, import.meta.url), "utf8").catch(() => "");
    if (!log.trim()) {
      missing.push(`${value} -> ${runId}`);
    }
  }
  assert.deepStrictEqual(missing, []);
});

/**
 * The one fixture that carries a payload instead of projecting one, held to
 * what the host would actually serve.
 *
 * `concurrent-run` exists because a concurrent group has geometry and the
 * projector that places it is on the other side of the two-host bundle
 * boundary. That makes it the one fixture that could drift from `buildState` —
 * so it is rebuilt here, through the same `extension.mjs` the canvas runs, and
 * any difference fails. `regenerate-concurrent-state.mjs` writes the file this
 * compares against, and both call the same builder, so the check cannot quietly
 * diverge from the thing it checks.
 */
test("the committed concurrent-group payload is the one the host builds", async () => {
  const rebuilt = await buildConcurrentState();

  assert.deepStrictEqual(
    { state: rebuilt, hostOwned: PROJECTED_FIELDS.filter((field) => field in CONCURRENT_STATE) },
    { state: CONCURRENT_STATE, hostOwned: [] },
  );
});

/**
 * The group family is the payload's whole reason for existing, so the payload
 * has to keep containing it. A pipeline edited down to plain steps would leave
 * both probes waiting on a card that is never drawn — a timeout rather than a
 * statement about the canvas.
 */
test("the concurrent-group payload draws a group with members and a run to fill it", async () => {
  const pipeline = CONCURRENT_STATE.pipelines?.["review-queue-pipeline"];
  const steps = pipeline?.layout?.steps ?? [];
  const log = await readFile(new URL("./fixtures/runs/run-group/events.jsonl", import.meta.url), "utf8");
  const events = log.trim().split("\n").map((line) => JSON.parse(line));
  const group = events.find((event) => event.kind === "group_started");

  assert.deepStrictEqual(
    {
      groupStep: steps.filter((step) => step.kind === "concurrent").map((step) => step.name),
      members: steps.filter((step) => step.parentId).map((step) => step.name).sort(),
      container: (pipeline?.containers ?? []).map((item) => item.parent),
      logGroup: group?.data?.group,
      logMembers: [...(group?.data?.members ?? [])].sort(),
      // The members must disagree, or "an outcome per member" is a claim one
      // shared verdict would satisfy.
      outcomes: events.filter((event) => event.kind === "group_member_finished").map((event) => event.data.result.outcome).sort(),
      // Live backfills the whole log, so the group has to be finished in it —
      // and the run must not be, or the live picker will not list it.
      groupFinished: events.some((event) => event.kind === "group_finished"),
      runFinished: events.some((event) => event.kind === "run_finished"),
    },
    {
      groupStep: ["run-checks"],
      members: ["read-diff", "read-tests"],
      container: ["run-checks"],
      logGroup: "run-checks",
      logMembers: ["read-diff", "read-tests"],
      outcomes: ["failure", "success"],
      groupFinished: true,
      runFinished: false,
    },
  );
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

/**
 * A declared value that no state renders is only honest if a named rule turns
 * it away. Without this, the "excluded rather than missing" convention is
 * convention alone: a value could be added, never rendered and never rejected,
 * and every other assertion here would still pass because the balance is an
 * identity of the walk over whatever values happen to be declared.
 *
 * It does not catch a screen that was never declared at all — nothing pure can
 * — but it does hold every declared one to one of two fates, which is what the
 * three `save`/`save-error` axes depend on being true.
 */
test("every dimension value is either rendered by a state or refused by a named rule", () => {
  const rendered = new Set(STATES.flatMap((state) => ORDER.map((key) => `${key}:${state.dimensions?.[key]}`)));
  const stranded = DIMENSIONS.flatMap((dimension) => dimension.values
    .filter((value) => !rendered.has(`${dimension.id}:${value.id}`))
    .filter((value) => !refusedByRule(dimension.id, value.id))
    .map((value) => `${dimension.id}:${value.id}`));
  assert.deepStrictEqual(stranded, []);
});

/** Whether some rule reading this dimension rejects the value on every tuple. */
function refusedByRule(dimension, valueId) {
  const rules = CONSTRAINTS.filter((rule) => rule.reads.includes(dimension));
  const sample = (rotation) => Object.fromEntries(ORDER.map((key, index) => {
    const values = valuesOf(key).map((value) => value.id);
    return [key, key === dimension ? valueId : values[(rotation + index) % values.length]];
  }));
  const rotations = Math.max(...ORDER.map((key) => valuesOf(key).length));
  const draws = Array.from({ length: rotations }, (_, rotation) => sample(rotation));
  return rules.some((rule) => draws.every((combo) => !rule.holds(combo)));
}

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

/**
 * The defect that made this shape necessary, held so it cannot come back.
 *
 * A whole-body substring cannot tell a word from its own negation: "unsaved
 * edits" contains "saved", so `edit: rest` — the state whose entire subject is
 * that nothing is pending — was satisfied by the editor reporting that
 * something was. Both halves are asserted here, because only the pair is the
 * claim: the loose form still passes the screen it should never have passed,
 * and the scoped form fails it by name.
 */
test("a scoped copy expectation tells a status from its own negation", () => {
  const dirty = {
    counts: { ".editor-status": 1 },
    texts: { ".editor-status": "unsaved edits" },
    paint: { ".editor-status": { ink: true, injected: "" } },
    text: "Pipeline editor unsaved edits",
    viewport: { width: 1280, height: 900 },
    overflowX: 0,
    contrast: [],
    boxes: [],
  };
  const loose = verdict({ expect: { shows: [], hides: [], copy: ["saved"] } }, dirty);
  const scoped = verdict({ expect: { shows: [], hides: [], copy: [{ selector: ".editor-status", text: "saved" }] } }, dirty);

  assert.deepStrictEqual([loose, scoped], [
    [],
    [{ kind: "missing-copy", detail: '.editor-status reads exactly “saved”' }],
  ]);
});

/**
 * And the same expectation passes the screen it is about, so the scoped form
 * is not simply a check that never holds.
 */
test("a scoped copy expectation passes the element it names", () => {
  const clean = {
    counts: { ".editor-status": 1 },
    texts: { ".editor-status": " Saved " },
    paint: { ".editor-status": { ink: true, injected: "" } },
    text: "Pipeline editor saved",
    viewport: { width: 1280, height: 900 },
    overflowX: 0,
    contrast: [],
    boxes: [],
  };

  assert.deepStrictEqual(verdict({ expect: { shows: [], hides: [], copy: [{ selector: ".editor-status", text: "saved" }] } }, clean), []);
});

/**
 * A promised sentence is painted, and painted as itself.
 *
 * Every scoped copy expectation above is settled by `texts`, and `texts` is
 * `innerText` — which answers for the DOM rather than for a reader. `innerText`
 * reports words drawn in transparent ink, and it does not report a `::before` or
 * `::after` at all. So a stylesheet that colours the panel's own sentence
 * `transparent` and spells a different one in generated content leaves
 * `innerText` exactly right, and with it every gate: 681 browser checks and 452
 * offline ones stayed green while the panel showed a config `bureau validate`
 * had *rejected* as `clean — bureau validate would pass`. The promise was kept
 * in the DOM and broken on the screen, which is the one failure a review surface
 * may not have — the gallery would show the false sentence and the matrix would
 * call the state correct.
 *
 * This is the family `visible()` already defends against, one level down. That
 * walk asks whether a promised element paints *anything*; these rows ask whether
 * what it paints is its *own words*.
 *
 * Every row keeps `texts` truthful, because that is the entire point: the
 * expectation the registry states is satisfied and the render is still a lie.
 * The first row is the vacuity guard — a mark that fired on an honest render
 * could never be kept green. The middle rows pin that the two clauses are
 * independent, since either alone is half a check. The last refuses to call an
 * absent paint sample readable.
 */
test("a promised sentence has to be painted, and painted as itself", () => {
  const kinds = (paint) => verdict(
    { expect: { shows: [], hides: [], copy: [{ selector: SELECTORS.panelValidationClean, text: PANEL_ELSEWHERE }] } },
    {
      counts: { [SELECTORS.panelValidationClean]: 1 },
      texts: { [SELECTORS.panelValidationClean]: PANEL_ELSEWHERE },
      paint: { [SELECTORS.panelValidationClean]: paint },
      text: PANEL_ELSEWHERE,
      viewport: { width: 1280, height: 900 },
      overflowX: 0,
      contrast: [],
      boxes: [],
    },
  ).map((item) => item.kind);

  assert.deepStrictEqual(
    [
      kinds({ ink: true, injected: "" }),
      kinds({ ink: false, injected: "" }),
      kinds({ ink: true, injected: `"${PANEL_CLEAN}"` }),
      kinds({ ink: false, injected: `"${PANEL_CLEAN}"` }),
      kinds(undefined),
    ],
    [[], ["unreadable-copy"], ["substituted-copy"], ["unreadable-copy", "substituted-copy"], ["unreadable-copy"]],
  );
});

/**
 * A scoped expectation is only checkable if its element was gathered, and
 * `collect` gathers exactly the selectors it is handed. So the selector list a
 * state produces has to include the ones its copy names — otherwise the text
 * would be `undefined`, and the expectation would fail on every render for a
 * reason that has nothing to do with the screen.
 */
test("a state's selector list covers the elements its copy names", () => {
  const scoped = STATES.filter((state) => (state.expect.copy ?? []).some((phrase) => typeof phrase === "object"));
  const uncovered = scoped.flatMap((state) => (state.expect.copy ?? [])
    .filter((phrase) => typeof phrase === "object" && !selectorsFor(state).includes(phrase.selector))
    .map((phrase) => `${state.id} -> ${phrase.selector}`));

  assert.deepStrictEqual([scoped.length > 0, uncovered], [true, []]);
});

/**
 * Selectors the vocabulary defines but the registry deliberately does not
 * promise, each with the reason it is exempt.
 *
 * An allow-list rather than silence, because the two are not the same claim: a
 * name in here is a decision someone can disagree with, and a name that is
 * merely absent is an accident nothing can see. It is empty, and that is the
 * intended resting state — every control the vocabulary names is promised by
 * some render.
 */
/**
 * Every selector in the vocabulary is claimed by some state, or exempt by name.
 *
 * This exists because the defect it catches has now happened five times. A
 * selector defined in `selectors.mjs` and read by no state is a control the
 * whole matrix is blind to: `pipelineBack`, `pipelineEditLink` and `editorBack`
 * were the doors out of the viewer and the editor, `pipelineRef` was the door
 * in — the one control carrying the assignment-first mental model — and
 * deleting any of them left all 500 renders green while the surface became a
 * room with no door. `status` was the fifth, found by review rather than by a
 * test, which is the point: the previous four were fixed by hand and nothing
 * was added that would notice the next one.
 *
 * A claim has to be a *positive* one: `shows`, a scoped `copy` selector, or an
 * operation in an entry path or a transition delta. `hides` deliberately does
 * not count, and that is the sixth telling of the same defect. An absence is
 * satisfied by deleting the control everywhere — a selector only ever named in
 * `hides` is one the product could stop drawing entirely with every render
 * staying green, which is precisely the blindness this test exists to refuse.
 * It found four: `editorShell`, `workSourceExact`, `liveCountZero` and
 * `deleteRefusedResting`, each of which had a state asserting it absent and
 * none asserting it present, so all four denials held against nothing.
 */
const UNPROMISED = {};

test("every selector the vocabulary defines is promised by a state, or exempt by name", () => {
  const claimed = new Set(STATES.flatMap((state) => [
    ...(state.expect.shows ?? []),
    ...(state.expect.copy ?? []).filter((phrase) => typeof phrase === "object").map((phrase) => phrase.selector),
    ...selectorsOf(state.ops),
  ]).concat(TRANSITIONS.flatMap((edge) => selectorsOf(edge.delta))));

  const unclaimed = Object.entries(SELECTORS)
    .filter(([name, selector]) => !claimed.has(selector) && !(name in UNPROMISED))
    .map(([name, selector]) => `${name} (${selector})`);
  const staleExemption = Object.keys(UNPROMISED).filter((name) => !(name in SELECTORS) || claimed.has(SELECTORS[name]));

  assert.deepStrictEqual({ unclaimed, staleExemption }, { unclaimed: [], staleExemption: [] });
});

function selectorsOf(ops) {
  return (ops ?? []).map((op) => op.selector).filter(Boolean);
}

/**
 * A verdict is a result, and a result requires a run.
 *
 * The pipeline panel says a sentence when it has no findings for this
 * pipeline, and an empty findings list arrives two different ways. Under
 * `data:validated` the CLI ran and returned nothing, which is a pass. Under
 * `data:fixture` there is no bureau binary, nothing ever ran, and the list is
 * empty for want of anyone to fill it — on the same screen whose header says
 * "Showing bundled sample; bureau binary not available."
 *
 * Both were wired to "clean — bureau validate would pass", so this registry
 * certified a verdict no validator gave, across seven states and fourteen
 * renders, in the state a user lands on precisely when the binary is missing.
 * That is a mark standing in for a check that was never made — the shape this
 * branch exists to refuse — sitting inside the branch's own expectations.
 *
 * Asked of the two values' own derivations rather than against a pair of
 * literals, because what is under test is that they *disagree*: a test naming
 * both sentences goes on agreeing with itself after someone makes them one
 * sentence again. The direction is pinned too — which of the two is the unrun
 * one — since a check for "they differ" alone is satisfied by swapping them,
 * and swapped is the same overclaim with the labels exchanged.
 */
test("a state where validate never ran does not claim the verdict of one where it did", () => {
  const said = (id) => (valueOf("data", id).derive({ surface: "pipeline" }).copy ?? []).map((phrase) => phrase.text);
  const [unchecked, clean] = [said("fixture"), said("validated")];

  assert.deepStrictEqual(
    [unchecked.length, clean.length, unchecked[0] === clean[0], /^not checked\b/u.test(unchecked[0]), /^clean\b/u.test(clean[0])],
    [1, 1, false, true, true],
  );
});

/**
 * The same defect one axis over: `validated` is not a verdict either.
 *
 * `lib/findings.mjs` sets `state: "validated"` whenever `bureau validate`
 * returned JSON — accepted *and* rejected — and records which in `ok`. A
 * verdict reading only `state` therefore says "clean" for a config the command
 * rejected, and the panel's list is scoped to the open pipeline, so any
 * rejection naming a role, an assignment, `repos.yaml` or another pipeline
 * lands exactly there: empty list, `ok: false`, and a header on the same screen
 * reading "Validation findings".
 *
 * The unrun case is included so all three sentences are pinned by one table.
 * `{ state: "fixture", ok: true }` is the shape that makes the point — `ok` is
 * true and nothing ran, so a rule reading `ok` alone would be as wrong as the
 * one reading `state` alone, in the other direction.
 */
test("an empty findings list carries the verdict the run actually produced", () => {
  const cases = [
    [undefined, PANEL_UNCHECKED],
    [{ state: "fixture", ok: true }, PANEL_UNCHECKED],
    [{ state: "crash", ok: false }, PANEL_UNCHECKED],
    [{ state: "validated", ok: true }, PANEL_CLEAN],
    [{ state: "validated", ok: false }, PANEL_ELSEWHERE],
  ];

  assert.deepStrictEqual(cases.map(([validation]) => emptyVerdict(validation)), cases.map(([, sentence]) => sentence));
});

/**
 * The three sentences stay three, and each stays the one it is.
 *
 * Sharing one export between the page and the registry is what stops them
 * drifting apart — but it also means the table above compares `emptyVerdict`'s
 * output against the very constants it returns, so both sides move together and
 * the *wording* is asserted only against itself. Two of the three pairs happen
 * to be pinned elsewhere: `unchecked` against `clean` by the derivation test,
 * and `elsewhere` against `clean` by the direction column below. The third pair
 * was pinned nowhere, so setting `PANEL_ELSEWHERE` to the unchecked sentence
 * left every gate green — offline, browser and gallery alike — while the panel
 * told a reader that a config the CLI *did* check and *did* reject "was not
 * checked". That is the original defect with the labels exchanged, which is
 * exactly what the derivation test's own comment warns a difference check alone
 * cannot catch.
 *
 * So distinctness is asserted over the set, and each sentence is held to its own
 * opening besides: a set of three survives any swap, and the openings are what
 * make a swap fail.
 *
 * An opening is *only* what it says it is, though. Each of these sentences puts
 * its subject first and its verdict after the dash, so a pin on the opening
 * holds the subject and leaves the answer free — which is the half a reader
 * takes the verdict from. That is the next test, not this one.
 */
test("the three verdicts stay three distinct sentences", () => {
  assert.deepStrictEqual(
    [
      new Set([PANEL_UNCHECKED, PANEL_CLEAN, PANEL_ELSEWHERE]).size,
      /^not checked\b/u.test(PANEL_UNCHECKED),
      /^clean\b/u.test(PANEL_CLEAN),
      /^no findings for this pipeline\b/u.test(PANEL_ELSEWHERE),
    ],
    [3, true, true, true],
  );
});

/**
 * Each sentence carries the answer it is *for*, and neither of the other two.
 *
 * Distinctness and the openings above are both satisfied by a sentence that
 * reverses its own meaning. `PANEL_ELSEWHERE` opens "no findings for this
 * pipeline" and its verdict is the clause after the dash, so rewriting that
 * clause to "bureau validate accepted the config" kept the set at three, kept
 * the opening, and left every gate green — offline, browser and gallery alike —
 * while the panel reported a config the CLI had *rejected* as one it had
 * accepted. That is round twenty-two's defect exactly, surviving round
 * twenty-two's fix, because the fix pinned where each sentence starts and the
 * defect lives in where it ends.
 *
 * The reason a pin here is worth anything at all is that these three clauses are
 * spelled *in this file* rather than imported. Every other comparison in this
 * suite reads `PANEL_*` — necessarily, since sharing the export is what stops
 * the page and the registry drifting — and a comparison against the constant is
 * a comparison the constant wins by definition. These literals are the only
 * independent statement of what the sentences must mean, so they are the only
 * thing a reversal can fail against.
 *
 * Asserted as an exact partition rather than three `includes` checks: each
 * sentence must carry its own clause *and* neither of the others, so a verdict
 * moved onto the wrong sentence fails on the row it arrived at as well as the
 * one it left. Rewriting a clause away entirely leaves that sentence carrying
 * none, which is a different failure and not a pass.
 */
test("each verdict says which answer the validator gave, and not another's", () => {
  const clauses = { unchecked: "did not run", clean: "would pass", elsewhere: "rejected the config" };
  const sentences = { unchecked: PANEL_UNCHECKED, clean: PANEL_CLEAN, elsewhere: PANEL_ELSEWHERE };
  const carried = Object.values(sentences).map((sentence) =>
    Object.keys(clauses).filter((kind) => sentence.includes(clauses[kind])),
  );

  assert.deepStrictEqual(carried, [["unchecked"], ["clean"], ["elsewhere"]]);
});

/**
 * The page asks the rule, rather than saying it again itself.
 *
 * `panel-verdict.mjs` is pure and fully pinned above, and none of that reaches a
 * reader unless `SidePanel` actually calls it. Nothing asserted that it did:
 * putting the "clean" sentence back inline — the literal this module was
 * extracted to delete — restored the original false verdict with all 450 offline
 * tests green. `web-imports.test.mjs` sees the import, but an import is not a
 * call, and a module can be imported and ignored.
 *
 * Read from source because `app.mjs` cannot be imported without a browser: it
 * takes React and `@xyflow/react` through bare specifiers. The browser matrix
 * does render these states and would catch a reverted call — but only the seven
 * `surface:pipeline` states that reach a panel, six minutes and a Chromium
 * later. A rule about which module owns a sentence is answerable from the text,
 * and answering it here is what makes the offline suite a real floor under it.
 *
 * Both directions, because either alone is half a check. The call must be there,
 * and the sentences must *not* be — an `emptyVerdict` call left beside a second
 * inline copy is exactly the drift the extraction was for, and the call clause
 * alone would pass it.
 */
test("the panel takes its empty-list sentence from the rule instead of spelling one", async () => {
  const source = await readFile(new URL("../web/app.mjs", import.meta.url), "utf8");

  assert.deepStrictEqual(
    [
      source.includes('import { emptyVerdict } from "./panel-verdict.mjs";'),
      source.includes("emptyVerdict(state.validation)"),
      [PANEL_UNCHECKED, PANEL_CLEAN, PANEL_ELSEWHERE].filter((sentence) => source.includes(sentence)),
    ],
    [true, true, []],
  );
});

/**
 * The state asserts its own premise, or it is green for the uninteresting
 * reason.
 *
 * `invalid-elsewhere` only exercises the `validated && !ok` branch while its
 * findings name nothing on the pipeline the fixture opens. If a later edit
 * attached one to that pipeline, the panel would fill, the branch would stop
 * rendering, and the state would keep passing while covering nothing — so the
 * emptiness is checked here rather than assumed.
 *
 * The registry's sentence is compared against `emptyVerdict` applied to this
 * fixture's own validation record, not against a literal. A test naming the
 * sentence twice agrees with itself; only running the product's rule over the
 * product's payload binds the expectation to the page. The last column pins the
 * direction, since "not the clean one" is what collapsing the three sentences
 * back into two would break first.
 */
test("a rejection that names nothing here is reported as a rejection, not a pass", async () => {
  const base = await servedState();
  const state = applyFixture(["invalid-elsewhere", "pipeline"], base);
  const open = state.selectedPipeline.name;
  const here = (state.findings ?? []).filter((finding) => (finding.target ?? {}).pipeline === open);
  const said = (valueOf("data", "invalid-elsewhere").derive({ surface: "pipeline" }).copy ?? []).map((phrase) => phrase.text);

  assert.deepStrictEqual(
    [state.validation.state, state.validation.ok, here.length, said[0] === emptyVerdict(state.validation), said[0] === PANEL_CLEAN],
    ["validated", false, 0, true, false],
  );
});

/**
 * Which look answers when the settle window runs out.
 *
 * Table-driven because the rule is a decision over three booleans, and each row
 * is a real render the matrix produced. The two that matter are the last pair:
 * a failure that flickers is the contended worker and an observed clean look
 * should win, while a failure that has lasted as long as agreement itself
 * requires is the product and must be reported — that second row is the one
 * that was green before, and it is how a control disappearing after first paint
 * used to pass all 500 renders.
 */
test("a render that never settled is answered by the look that is telling the truth", () => {
  const cases = [
    // Nothing wrong at the deadline: the last look answers, clean.
    [{ lastFailed: false, sustained: 0, sawClean: true }, "last"],
    // Never once clean: the last look is the one carrying the failures to read.
    [{ lastFailed: true, sustained: 9, sawClean: false }, "last"],
    // Failing for as long as agreement needs. Sustained, so the product.
    [{ lastFailed: true, sustained: 3, sawClean: true }, "last"],
    // Failing on the last look only. Flickering, so the clean look answers.
    [{ lastFailed: true, sustained: 1, sawClean: true }, "clean"],
  ];

  assert.deepStrictEqual(
    cases.map(([observation]) => deadlineVerdict(observation, 3)),
    cases.map(([, expected]) => expected),
  );
});

/**
 * The other half of settling: a still page whose graph has not drawn yet.
 *
 * Stability alone was not enough. React Flow lays its edges out in a pass after
 * it has measured the nodes, and between the nodes landing and that pass
 * starting there is a real lull — long enough for three agreeing samples. So a
 * render could be filed settled on a graph of disconnected boxes, the gallery
 * published it for review, and the twin audit compared it as evidence: two
 * states declared twins were reported as no longer drawing the same screen when
 * one had drawn its four relation edges and the other had not.
 *
 * Table-driven over what a graph declared against what it has drawn. The empty
 * row is the one that keeps this usable: a state with no graph on screen, or a
 * graph with nothing to join, settles on stability alone as before.
 */
test("a render is settled only once every graph on it has drawn its edges", () => {
  const cases = [
    [[], true],
    [[{ declared: 0, drawn: 0 }], true],
    [[{ declared: 4, drawn: 4 }], true],
    [[{ declared: 4, drawn: 0 }], false],
    [[{ declared: 4, drawn: 4 }, { declared: 2, drawn: 1 }], false],
  ];

  assert.deepStrictEqual(
    cases.map(([graphs]) => graphsDrawn({ graphs })),
    cases.map(([, expected]) => expected),
  );
});

/** A snapshot from before the rule existed reads as drawn, not as unfinished. */
test("a snapshot that files no graphs is not held back by the rule", () => {
  assert.deepStrictEqual([graphsDrawn({}), graphsDrawn(undefined)], [true, true]);
});

/**
 * Settling is stability *and* a finished edge pass, and both halves belong to
 * both consumers.
 *
 * Only the second half was ever shared. The matrix required `SETTLE_REPEATS`
 * consecutive looks with an unchanged signature; the lab required nothing and
 * left on the first failure-free look, so the surface a human reviews certified
 * renders the run that gates CI marked. `transport:playing` is the case that
 * makes it concrete: its scrubber advances every 100ms, its signature never
 * holds still, and the two surfaces gave a reviewer opposite answers about the
 * same render.
 *
 * Table-driven over one run of looks, because what is under test is a fold: an
 * unchanged signature has to accumulate, a changed one has to reset the count
 * to zero rather than decrement it, and the threshold is reached on the look
 * where agreement — not sampling — reaches `SETTLE_REPEATS`.
 */
test("a render is settled once its signature has held still and its graphs have drawn", () => {
  const looks = ["a", "a", "a", "a", "b", "b"];
  const walked = [];
  let settle = null;
  for (const signature of looks) {
    settle = settleStep(settle, { signature, graphs: [{ name: "g", declared: 2, drawn: 2 }] });
    walked.push([settle.agreed, settle.settled]);
  }

  assert.deepStrictEqual(walked, [[0, false], [1, false], [2, false], [3, true], [0, false], [1, false]]);
});

/** A held signature over a graph still drawing is not settled either. */
test("a still signature does not settle a render whose graph has not finished", () => {
  const drawing = { signature: "a", graphs: [{ name: "g", declared: 2, drawn: 0 }] };
  let settle = null;
  for (let index = 0; index < 6; index += 1) {
    settle = settleStep(settle, drawing);
  }

  assert.deepStrictEqual([settle.agreed >= 3, settle.settled], [true, false]);
});

/**
 * Why a render was not proved settled, said in words that are true of it.
 *
 * The note read "a graph on this render has not drawn all of its edges"
 * whenever `settled` was false. That was accurate while `settled` meant the
 * edge pass alone, and became a false statement the moment stability joined it:
 * a replay whose scrubber is advancing has drawn every edge it declared, and
 * sending a reviewer to inspect its graph sends them after a defect that is not
 * there. Three causes, three sentences.
 */
test("an unsettled render is told the reason that actually applies to it", () => {
  const drawn = { graphs: [{ name: "g", declared: 2, drawn: 2 }] };
  const behind = { graphs: [{ name: "g", declared: 2, drawn: 0 }] };
  const named = [{ kind: "undrawn-graph", name: "g", detail: "g declared 2 edge(s) and drew 0" }];

  assert.deepStrictEqual(
    [
      unsettledReason(behind, named),
      unsettledReason(behind, []).includes("too few looks"),
      unsettledReason(drawn, []).includes("never stopped changing"),
    ],
    ["g declared 2 edge(s) and drew 0", true, true],
  );
});

/**
 * Waiting for the edge pass is not the same as reporting that it never came.
 *
 * `graphsDrawn` decides when a render is finished, and for a while that was all
 * the count did: a graph that would never draw held the loop to its deadline
 * and then left with `settled: false` — an amber mark whose sentence is about
 * the harness ("this frame may have raced"), not about the screen. Nothing
 * failed. So the one condition the count exists to detect could hold on every
 * run of every state and the matrix would still be green, which is the exact
 * shape of defect this registry is written against: a mark standing in for a
 * check that found something.
 *
 * The rule names the graph and both numbers, because "a graph did not finish"
 * is not actionable and "the relation graph declared 3, drew 1" is. Three
 * surfaces publish the count and the editor can have two of them on one render,
 * so the numbers alone do not say which screen to go and look at.
 */
test("a graph that never drew its edges is named as a failure, not a note", () => {
  assert.deepStrictEqual(
    undrawnGraphs({
      graphs: [
        { name: "editor-flow", declared: 4, drawn: 4 },
        { name: "Config relation graph", declared: 3, drawn: 1 },
      ],
    }),
    [{
      kind: "undrawn-graph",
      name: "Config relation graph",
      detail: "Config relation graph declared 3 edge(s) and drew 1",
    }],
  );
});

/** A snapshot from before graphs carried a name still reads as a sentence. */
test("an unnamed graph is still reported", () => {
  assert.deepStrictEqual(
    undrawnGraphs({ graphs: [{ declared: 2, drawn: 0 }] }).map((finding) => finding.detail),
    ["a graph declared 2 edge(s) and drew 0"],
  );
});

/**
 * The barrier and the report are two questions over one count.
 *
 * `graphsDrawn` is an `every`, so a render with no graph on it yet answers
 * "nothing is behind" — correct for deciding whether to keep waiting, and wrong
 * for deciding whether the draw pass ever happened. The look that parts them is
 * the one taken between a `<details>` toggle and React committing the graph,
 * and it is reachable on every state that opens one.
 *
 * Counted per graph, because a render's graph set changes while it is being
 * sampled and the thing being excused is a graph. A render carrying none
 * contributes no runs, so it still costs nothing.
 */
test("a look is counted per graph, and carried across one it is absent from", () => {
  const first = undrawnLooks(new Map(), { graphs: [{ name: "a", declared: 4, drawn: 0 }, { name: "b", declared: 2, drawn: 2 }] });
  const second = undrawnLooks(first, { graphs: [{ name: "a", declared: 4, drawn: 4 }, { name: "b", declared: 2, drawn: 0 }] });
  const gone = undrawnLooks(second, { graphs: [] });

  assert.deepStrictEqual(
    [first.get("a"), second.get("a"), gone.get("a"), gone.get("b"), [...undrawnLooks(new Map(), undefined)]],
    [
      { looks: 1, missed: 1, run: 1 },
      { looks: 2, missed: 1, run: 0 },
      { looks: 2, missed: 1, run: 0 },
      { looks: 2, missed: 1, run: 1 },
      [],
    ],
  );
});

/**
 * A graph that drew does not excuse a different graph that never did, does not
 * excuse *itself* forever, and cannot buy the exemption back by flashing or by
 * unmounting.
 *
 * Every simpler shape had the defect somewhere else in it. A flag per render
 * let `.editor-flow` answer for the `.relation-flow` that mounted behind it. A
 * run of consecutive looks was reset by any complete look, so a graph flashing
 * on and off never reached the threshold. A bare total was never reset, and so
 * was spent by two harmless early relayouts — turning one ordinary late miss
 * into a hard failure on the animating states that must never fail for it. So
 * two questions are asked of three numbers: did it break and stay broken, and
 * was it broken for a material share of the time it was on screen.
 */
test("a graph is failed for breaking and staying broken, or for breaking chronically", () => {
  const broken = { graphs: [{ name: "g", declared: 4, drawn: 0 }] };
  const cases = [
    [{ looks: 9, missed: 3, run: 3 }, 1],
    [{ looks: 50, missed: 25, run: 1 }, 1],
    [{ looks: 50, missed: 3, run: 1 }, 0],
    [{ looks: 3, missed: 2, run: 2 }, 0],
    [undefined, 0],
  ];

  assert.deepStrictEqual(
    cases.map(([tally]) => undrawnFor(broken, new Map(tally ? [["g", tally]] : []), 3).length),
    cases.map(([, expected]) => expected),
  );
});

/**
 * The two sequences the rule exists to tell apart, folded rather than asserted
 * as tallies, so the fold and the verdict are held together.
 *
 * A graph blinking on and off for the whole budget is the case a run could not
 * reach; two early relayouts and one late miss is the case a bare total called
 * broken. Both end incomplete, and only the first is a screen.
 */
test("flashing all budget fails where a few scattered relayouts do not", () => {
  const on = { graphs: [{ name: "g", declared: 4, drawn: 4 }] };
  const off = { graphs: [{ name: "g", declared: 4, drawn: 0 }] };
  const fold = (pick) => {
    let looks = new Map();
    for (let index = 0; index < 50; index += 1) {
      looks = undrawnLooks(looks, pick(index) ? off : on);
    }
    return undrawnFor(off, looks, 3).length;
  };

  assert.deepStrictEqual(
    [fold((index) => index % 2 === 1), fold((index) => index < 2 || index === 49)],
    [1, 0],
  );
});

/**
 * Unmounting is not a reset either. A graph alternating between absent and
 * incomplete accumulated nothing while the tally was rebuilt from each look,
 * so a surface blinking in and out of the document for the whole budget was
 * exempt — the same escape as the others, through the one door left open.
 */
test("a graph that blinks in and out of the document is not exempt", () => {
  const off = { graphs: [{ name: "g", declared: 4, drawn: 0 }] };
  let looks = new Map();
  for (let index = 0; index < 50; index += 1) {
    looks = undrawnLooks(looks, index % 2 === 0 ? { graphs: [] } : off);
  }

  assert.deepStrictEqual([looks.get("g"), undrawnFor(off, looks, 3).length], [{ looks: 25, missed: 25, run: 25 }, 1]);
});

/**
 * And the tolerance all of this exists to preserve. A graph complete on the
 * final look is reported by nothing, whatever its history — which is what keeps
 * a healthy surface caught mid-relayout on the way past from being an
 * accusation, and what lets the thresholds be strict without being flaky.
 */
test("a graph that finished is not accused of how raggedly it got there", () => {
  assert.deepStrictEqual(
    undrawnFor({ graphs: [{ name: "g", declared: 4, drawn: 4 }] }, new Map([["g", { looks: 50, missed: 40, run: 0 }]]), 3),
    [],
  );
});

/**
 * The states that animate by design must not fail for it.
 *
 * `transport:playing` advances a scrubber on an interval, so its signature
 * never goes still and it reaches the deadline on every run — but its graph
 * draws its edges like any other. Reporting an unsettled render as an undrawn
 * graph would light this rule on exactly the states it has nothing to say
 * about, so the question is asked of the graphs and never of the stability.
 */
test("a page that never stops moving is not accused of an undrawn graph", () => {
  const drawn = [{ name: "pipeline-flow", declared: 4, drawn: 4 }, { name: "editor-flow", declared: 0, drawn: 0 }];
  assert.deepStrictEqual([undrawnGraphs({ graphs: drawn }), undrawnGraphs({}), undrawnGraphs(undefined)], [[], [], []]);
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

/**
 * The three ways a control can have a box and still not be on screen.
 *
 * Each was invisible to the old verdict, which measured a fixed list of
 * regions and only ever asked whether one hung off the *right* edge. A save
 * button pushed off the left, or cut away entirely by its own editor, reported
 * a rect like any other and satisfied `shows`.
 */
test("the verdict catches a control off either edge or cut away by an ancestor", () => {
  const state = { expect: { shows: [], hides: [], copy: [] } };
  const boxes = [
    { selector: ".off-left", x: -40, y: 10, width: 100, height: 20 },
    { selector: ".off-right", x: 1200, y: 10, width: 300, height: 20 },
    { selector: ".cut-away", x: 10, y: 10, width: 100, height: 20, clipped: true },
  ];
  const snapshot = { counts: {}, text: "", viewport: { width: 1280, height: 900 }, overflowX: 0, contrast: [], boxes };

  assert.deepStrictEqual(verdict(state, snapshot).map((item) => item.detail).sort(), [
    ".cut-away is cut away entirely by a clipping ancestor",
    ".off-left starts 40px left of the viewport",
    ".off-right extends 220px past the viewport",
  ]);
});

/**
 * The same-kind rule, and the containment it must not mistake for a defect.
 *
 * `STACKED` compares every pair of boxes drawn by one selector, which was
 * written on the assumption that such a selector never nests inside itself. The
 * repo adder breaks that: its resolved preview draws a `.detail-row` per field
 * inside the `.detail-row` that holds the whole repos field, and a parent
 * always intersects its child. Five overlaps were reported on a screen that
 * renders correctly, so the rule now skips a pair when one box contains the
 * other — and only that pair, so two rows that really do print over each other
 * still fail.
 */
test("the same-kind overlap rule spares a row drawn inside another row", () => {
  const state = { expect: { shows: [], hides: [], copy: [] } };
  const row = (id, extra) => ({ selector: ".detail-row", id, x: 0, y: 0, width: 400, height: 40, parent: `parent-${id}`, flow: true, within: [], ...extra });
  const snapshot = (boxes) => ({ counts: {}, text: "", viewport: { width: 1280, height: 900 }, overflowX: 0, contrast: [], boxes });

  assert.deepStrictEqual(
    {
      nested: verdict(state, snapshot([row("node-0", { height: 200 }), row("node-1", { y: 40, within: ["node-0"] })])),
      overprinted: verdict(state, snapshot([row("node-0"), row("node-1", { y: 20 })])).map((item) => item.detail),
    },
    { nested: [], overprinted: [".detail-row #0 overlaps #1"] },
  );
});

/**
 * The graph's cards are the one region both halves of the overlap rule would
 * otherwise miss, and nothing required them to be covered.
 *
 * React Flow gives every node its own absolutely positioned wrapper, so
 * `.flow-card` boxes are not in normal flow and share no DOM parent: the
 * sibling rule skips them by construction, and no `SIBLINGS` pair names them.
 * Same-selector comparison is the only rule that can reach them, and it reaches
 * them only while `.flow-card` is in `STACKED` — while `MEASURED` is what
 * decides the boxes are collected at all.
 *
 * Both memberships were unasserted. Deleting `.flow-card` from either list left
 * every test in this file green while the matrix quietly stopped detecting two
 * pipeline cards drawn on top of each other — the collision that put a terminal
 * rail inside a concurrent group's member row.
 *
 * Asserted through the behaviour rather than by reading the lists back, so a
 * rule that keeps the name and stops acting on it fails too.
 */
test("two graph cards drawn on top of each other are caught, and their boxes are collected", () => {
  const state = { expect: { shows: [], hides: [], copy: [] } };
  // Absolutely positioned and parentless, exactly as React Flow draws them, so
  // this passes only if the same-selector rule is doing the work.
  const card = (id, extra) => ({ selector: ".flow-card", id, x: 0, y: 0, width: 200, height: 80, parent: null, flow: false, within: [], ...extra });
  const snapshot = (boxes) => ({ counts: {}, text: "", viewport: { width: 1280, height: 900 }, overflowX: 0, contrast: [], boxes });

  assert.deepStrictEqual(
    {
      collected: measureFor(state).includes(".flow-card"),
      apart: verdict(state, snapshot([card("node-0"), card("node-1", { x: 220 })])),
      overprinted: verdict(state, snapshot([card("node-0"), card("node-1", { x: 100 })])).map((item) => item.detail),
    },
    { collected: true, apart: [], overprinted: [".flow-card #0 overlaps #1"] },
  );
});

/**
 * The sibling rule, and the two things it must not mistake for a defect.
 *
 * `SIBLINGS` in `checks.mjs` is a hand-kept list of landing pairs; this is the
 * general form — anything in normal flow that prints over a box sharing its
 * parent. It has to ignore one element measured under two selectors (a `shows`
 * of `[data-testid=x]` and `[data-testid=x]:not([disabled])` is one button),
 * and it has to ignore absolutely positioned boxes, whose whole job is to sit
 * on top of something.
 */
test("the sibling overlap rule spares one element under two selectors, and anything positioned", () => {
  const state = { expect: { shows: [], hides: [], copy: [] } };
  const at = (selector, extra) => ({ selector, x: 0, y: 0, width: 100, height: 40, parent: "parent-0", flow: true, ...extra });
  const snapshot = (boxes) => ({ counts: {}, text: "", viewport: { width: 1280, height: 900 }, overflowX: 0, contrast: [], boxes });

  assert.deepStrictEqual(
    {
      sameElement: verdict(state, snapshot([at(".save", { id: "node-0" }), at(".save:not([disabled])", { id: "node-0" })])),
      positioned: verdict(state, snapshot([at(".card", { id: "node-0" }), at(".badge", { id: "node-1", flow: false })])),
      genuine: verdict(state, snapshot([at(".one", { id: "node-0" }), at(".two", { id: "node-1" })])).map((item) => item.detail),
    },
    { sameElement: [], positioned: [], genuine: [".one overlaps .two"] },
  );
});

/**
 * The measured set has to include what the state is actually about. Measuring
 * only the standing regions is how a clipped Save button passed: nothing eve
 * asked where its box was.
 */
test("a state is measured against its own expected controls, not just the standing regions", () => {
  const state = { expect: { shows: ['[data-testid="limits-save"]'], hides: [], copy: [] } };
  const measured = measureFor(state);

  assert.deepStrictEqual(
    { includesShown: measured.includes('[data-testid="limits-save"]'), keepsStanding: measured.includes(".assignment-card") },
    { includesShown: true, keepsStanding: true },
  );
});

test("the verdict refuses copy that reserves a region instead of drawing it", () => {
  // The pipeline side panel used to end on a constant "Trust flow — Reserved
  // for trust analysis." while the trust advisories it names were drawn in the
  // findings above it. A region that never varies has no state to assert, so
  // this is the one check that can fail for it.
  const state = { expect: { shows: [], hides: [], copy: [] } };
  const snapshot = {
    counts: {},
    text: "Legend\nTrust flow\nReserved for trust analysis.",
    viewport: { width: 1280, height: 900 },
    overflowX: 0,
    contrast: [],
    boxes: [],
  };
  assert.deepStrictEqual(verdict(state, snapshot), [
    { kind: "placeholder-copy", detail: 'the render says "reserved for" instead of drawing it' },
  ]);
});

/**
 * The create bar drew its refusal as an extra child of a four-column field
 * grid, so the Name *label* was pushed into the next cell and its input onto
 * the row below. Nothing was clipped, nothing overlapped, both controls were
 * present and the copy was right — the form simply named the wrong box. This is
 * the check that can fail for it, and the three arrangements below are the ones
 * the two viewports actually produce.
 */
test("the verdict catches a label that names a control it does not sit beside", () => {
  const state = { expect: { shows: [], hides: [], copy: [] } };
  const at = (labels) => ({ counts: {}, text: "", viewport: { width: 1280, height: 900 }, overflowX: 0, contrast: [], boxes: [], labels });
  const label = { x: 500, y: 200, width: 42, height: 20 };

  assert.deepStrictEqual(
    {
      beside: verdict(state, at([{ text: "Name", label, control: { x: 560, y: 198, width: 200, height: 30 } }])),
      above: verdict(state, at([{ text: "Name", label, control: { x: 500, y: 226, width: 200, height: 30 } }])),
      stranded: verdict(state, at([{ text: "Name", label, control: { x: 180, y: 255, width: 64, height: 30 } }])),
    },
    {
      beside: [],
      above: [],
      stranded: [{ kind: "stranded-label", detail: '"Name" sits neither beside nor above the control it names' }],
    },
  );
});

/**
 * `collect` is shipped to the page as source — `new Function` around its
 * `toString()`, in the browser suite and in the lab alike — so it may not
 * reference anything outside itself. Nothing enforced that, and the way it
 * fails is maximally unhelpful: a helper lifted to module scope leaves a
 * `ReferenceError` inside every single `page.evaluate`, so every render and
 * every transition go red at once and none of them names the cause.
 *
 * Rebuilding it the way the hosts do and running it over a document that
 * reaches every branch catches that offline, in milliseconds, and points at
 * this line. The document has to be a real one: a free variable is resolved
 * when the line reading it *runs*, so a stub that answers every query with an
 * empty list rebuilds cleanly no matter how much of `collect` has escaped —
 * which is what the first version of this test did, leaving eight of the nine
 * inner helpers unguarded.
 */
test("collect survives being rebuilt from its own source, as both hosts run it", () => {
  const doc = pageStub();
  const rebuilt = new Function(`return (${collect.toString()})`)();

  assert.deepStrictEqual(rebuilt(doc, { selectors: [".a", ".d", ".alpha", ".font", ".clip", ".indent", ".cover", ".occluded"], measure: [".b"], contrast: [".c"] }), {
    counts: { ".a": 2, ".d": 1, ".alpha": 1, ".font": 1, ".clip": 1, ".indent": 1, ".cover": 1, ".occluded": 1 },
    texts: {
      ".a": "saved wrapped",
      ".d": "no findings for this pipeline",
      ".alpha": "faint",
      ".font": "flat",
      ".clip": "cut away",
      ".indent": "far away",
      ".cover": "under a lid",
      ".occluded": "behind a sibling",
    },
    // `.d` is the render that keeps its promise in the DOM and breaks it on the
    // screen: `texts` is exactly the sentence a scoped expectation asks for, and
    // a reader sees the opposite one an `::after` paints over transparent ink.
    // Gathering both is what lets `verdict` tell the two apart.
    paint: {
      ".a": { ink: true, injected: "" },
      ".d": { ink: false, injected: '"clean — bureau validate would pass"' },
      ".alpha": { ink: false, injected: "" },
      ".font": { ink: false, injected: "" },
      ".clip": { ink: false, injected: "" },
      ".indent": { ink: false, injected: "" },
      ".cover": { ink: false, injected: "" },
      ".occluded": { ink: false, injected: "" },
    },
    boxes: [
      { selector: ".b", id: "node-0", x: 10, y: 10, width: 100, height: 20, parent: "parent-0", within: [], flow: true, clipped: false, trimmed: 0 },
      { selector: ".b", id: "node-1", x: 300, y: 10, width: 50, height: 20, parent: "parent-0", within: [], flow: true, clipped: true, trimmed: 150 },
      { selector: ".b", id: "node-2", x: 10, y: 60, width: 100, height: 80, parent: "parent-1", within: [], flow: true, clipped: false, trimmed: 40 },
    ],
    contrast: [{ selector: ".c", text: "Kind", ratio: 21 }],
    labels: [{
      text: "Name",
      label: { x: 0, y: 40, width: 60, height: 16 },
      control: { x: 70, y: 40, width: 120, height: 24 },
    }],
    signature: [
      "BUTTON|class=btn,data-testid=draft-save|Save||",
      "DIV|class=draft-bar|||",
      "INPUT|class=field,data-testid=create-name||release-pipeline|",
      "INPUT|class=toggle,data-testid=limit-on||on|checked",
      "DETAILS|class=relation-section,open=|||",
      "P|class=fallback-error|TypeError: Failed to fetch dynamically imported module: http://canvas.invalid/app.mjs||",
    ].join("\n"),
    graphs: [{ name: "relation-flow", declared: 2, drawn: 2 }],
    text: "Bureau",
    overflowX: 0,
    viewport: { width: 1280, height: 900 },
  });
});

const BASE_STYLE = {
  visibility: "visible",
  opacity: "1",
  display: "block",
  overflowX: "visible",
  overflowY: "visible",
  backgroundColor: "rgb(255, 255, 255)",
  color: "rgb(0, 0, 0)",
  webkitTextFillColor: "rgb(0, 0, 0)",
  fontSize: "14px",
  clipPath: "none",
  textIndent: "0px",
  position: "static",
  // What React Flow's stylesheet actually computes onto an edge path. Present
  // in the base style because "drawn" now means laid out *and* inked, so an
  // edge with no stroke declared anywhere would otherwise read as undrawn.
  stroke: "rgb(177, 177, 183)",
  strokeWidth: "1px",
  strokeOpacity: "1",
};

/** A document that holds nothing but the elements it is given. */
function docOf(nodes) {
  return {
    defaultView: { getComputedStyle: () => BASE_STYLE },
    documentElement: { clientWidth: 1280, clientHeight: 900, scrollWidth: 1280 },
    body: { innerText: "" },
    querySelectorAll: (selector) => (selector === "body *" ? nodes : []),
    getElementById: () => null,
  };
}

/** One element, as the signature walk reads it. */
function signed(className, text) {
  return {
    tagName: "DIV",
    attributes: [{ name: "class", value: className }],
    getAttribute: (name) => (name === "class" ? className : null),
    childElementCount: 0,
    textContent: text,
    getClientRects: () => [{ width: 40, height: 16 }],
    getBoundingClientRect: () => ({ x: 0, y: 0, width: 40, height: 16, left: 0, top: 0, right: 40, bottom: 16 }),
    contains: () => false,
    parentElement: null,
  };
}

/**
 * An edge caption's place in the document is the portal's business, not the
 * screen's.
 *
 * `EdgeLabelRenderer` puts every caption in one container in mount order, and
 * the `transform` that actually positions them is excluded from the signature
 * on purpose. So two paths to one screen could sign differently over nothing:
 * the tab round trip and the direct edit, declared twins and both proved
 * settled, were reported broken because `success` and `failure` had swapped
 * indices while both captions were on both screens.
 *
 * Order dropped, content kept — so a caption that is genuinely missing or
 * renamed still parts the two, which is the second and third case here.
 */
test("edge captions sign as a set, so a portal's mount order is not a difference", () => {
  const caption = (text) => signed("react-flow__edge-label edge-caption", text);
  const sign = (nodes) => collect(docOf(nodes), { selectors: [], measure: [], contrast: [] }).signature;
  const success = caption("success");
  const failure = caption("failure");
  const body = signed("step-card", "verify");

  assert.deepStrictEqual(
    [
      sign([body, success, failure]) === sign([body, failure, success]),
      sign([body, success, failure]) === sign([body, success]),
      sign([body, success, failure]) === sign([body, success, caption("blocked")]),
      sign([body, success]) === sign([signed("step-card", "review"), success]),
    ],
    [true, false, false, false],
  );
});

/**
 * The claim the fold actually makes: a state signs the same whichever port the
 * harness happened to bind, and still signs differently when the words change.
 *
 * Asserted as two comparisons rather than against a literal, because the literal
 * above already pins the shape. What was unfalsifiable before is the *pair* — a
 * signature carrying `127.0.0.1:40091` is perfectly well-formed, so nothing that
 * looked at one render alone could tell that the state could never match itself
 * on the next run, and `surface:boot+data:render-error` was a standing
 * broken-twin finding that no fix to the product would ever have cleared.
 */
test("a signature folds the harness's port away without folding the message away", () => {
  const signatureFor = (text) => {
    const doc = pageStub();
    const held = doc.querySelectorAll("body *");
    held[held.length - 1].textContent = text;
    return new Function(`return (${collect.toString()})`)()(doc, { selectors: [], measure: [], contrast: [] }).signature;
  };
  const onPort = (port) => `failed to import http://127.0.0.1:${port}/app.mjs`;

  assert.deepStrictEqual(
    [
      signatureFor(onPort(40091)) === signatureFor(onPort(35781)),
      signatureFor(onPort(40091)) === signatureFor("failed to import http://127.0.0.1:40091/editor.mjs"),
    ],
    [true, false],
  );
});

function boxOf(x, y, width, height) {
  return { x, y, width, height, top: y, left: x, right: x + width, bottom: y + height };
}

/** The rect list a node that occupies space reports. */
const AREA = [{ width: 100, height: 20 }];

/**
 * A document shaped so that each of `collect`'s inner helpers is called at
 * least once — `visible`, `keyFor`, `idFor`, `clipper`, `channels`, `opaque`,
 * `luminance`, `backdrop` and `rectOf`. Fidelity to a browser is not the
 * point; reaching the lines is.
 */
function pageStub() {
  const styles = new Map();
  const pseudos = new Map();
  const element = (style, own = {}) => {
    const node = {
      getClientRects: () => AREA,
      getBoundingClientRect: () => boxOf(0, 0, 0, 0),
      parentElement: null,
      textContent: "",
      getAttribute: () => null,
      // `collect` asks each measured node which other measured nodes contain
      // it, so a stub node has to be able to answer. Nothing here nests, which
      // is the case that must produce an empty `within`.
      contains: () => false,
      ...own,
    };
    styles.set(node, { ...BASE_STYLE, ...style });
    return node;
  };

  // A clipping ancestor, so `clipper` returns a rect and is asked about both
  // axes: one measured box sits inside it, one entirely past its right edge.
  const clip = element(
    { overflowX: "hidden", overflowY: "hidden" },
    { getBoundingClientRect: () => boxOf(0, 0, 200, 100) },
  );
  const inside = element({}, { parentElement: clip, getBoundingClientRect: () => boxOf(10, 10, 100, 20) });
  const past = element({}, { parentElement: clip, getBoundingClientRect: () => boxOf(300, 10, 50, 20) });
  // Zero-area, so the measure loop skips it and the ids stay contiguous.
  const collapsed = element({}, { parentElement: clip });

  // The two axes clipped by different ancestors, which is the ordinary shape of
  // a truncating label inside a lidded pane: the nearer wrapper hides overflow
  // on x only, the outer box on y only. A walk that stops at the first ancestor
  // clipping *either* axis reads this node's y overflow off the x-only wrapper,
  // finds it visible, and reports a clean box for content cut off below the lid.
  const lid = element({ overflowY: "hidden" }, { getBoundingClientRect: () => boxOf(0, 0, 200, 100) });
  const ellipsis = element({ overflowX: "hidden" }, { parentElement: lid, getBoundingClientRect: () => boxOf(0, 0, 200, 400) });
  const underLid = element({}, { parentElement: ellipsis, getBoundingClientRect: () => boxOf(10, 60, 100, 80) });

  // `visible` has to answer both ways over a node that still reports rects.
  // The painted one carries words and the unpainted one carries different
  // words, so the text gather is exercised *and* shown to skip what the reade
  // cannot see — a scoped copy expectation that read a hidden node would be
  // asserting the page's private state rather than its screen.
  const shown = element({}, { innerText: "saved" });
  const unpainted = element({ visibility: "hidden" }, { innerText: "unsaved edits" });
  // The other way a control measures perfectly and paints nothing. Two of
  // them, because they fail apart: the node's own `opacity: 0`, and a node
  // left fully opaque under a transparent parent — which is the case reading
  // the node alone cannot see, since opacity does not inherit into the
  // computed value the way `visibility` does. Both carry words, so a predicate
  // that misses either reports a count and a copy for a control the reader is
  // looking straight through.
  const transparent = element({ opacity: "0" }, { innerText: "invisible save" });
  const behindTransparent = element({}, {
    innerText: "invisible discard",
    parentElement: element({ opacity: "0" }),
  });
  // The third way: a control collapsed to no area at all. It reports a rect,
  // so counting rects called it shown; it covers no pixel, so a reader looking
  // at the screen would say the control is gone.
  const flattened = element({}, { innerText: "invisible cancel", getClientRects: () => [{ width: 0, height: 0 }] });
  // And the case that keeps the fix from being "every rect must have area": an
  // inline run broken across two lines reports an empty rect beside a real one,
  // and it is on screen. Requiring area of *every* rect would report an
  // ordinary wrapped label as missing.
  const wrapped = element({}, { innerText: "wrapped", getClientRects: () => [{ width: 0, height: 0 }, { width: 80, height: 20 }] });

  // Transparent over white, so `backdrop` must walk up and `opaque` answers
  // false then true before `luminance` runs on what it settles on.
  const painted = element({ backgroundColor: "rgb(255, 255, 255)" });
  const wording = element(
    { backgroundColor: "rgba(0, 0, 0, 0)" },
    { parentElement: painted, textContent: " Kind " },
  );
  const wordless = element({}, { parentElement: painted, textContent: "   " });

  // The way a promised sentence stays perfect in the DOM and false on screen.
  // `innerText` reports this node's own words and a reader sees none of them —
  // the ink is transparent — while an `::after` paints a different sentence
  // that `innerText` does not report at all. Both halves are here because
  // `collect` has to gather both: the ink, and the words put in their place.
  const ghostParent = element({});
  const ghost = element({ color: "rgba(0, 0, 0, 0)" }, {
    innerText: "no findings for this pipeline",
    parentElement: ghostParent,
  });
  pseudos.set(ghostParent, { "::after": { content: '"clean — bureau validate would pass"' } });
  const faint = element({ color: "rgba(0, 0, 0, 0.01)" }, { innerText: "faint" });
  const flat = element({ fontSize: "0px" }, { innerText: "flat" });
  const cutAway = element({ clipPath: "inset(100%)" }, { innerText: "cut away" });
  const farAway = element(
    { textIndent: "-9999px" },
    { innerText: "far away", getBoundingClientRect: () => boxOf(0, 0, 100, 20) },
  );
  const coverParent = element({});
  const covered = element({}, { innerText: "under a lid", parentElement: coverParent });
  pseudos.set(coverParent, {
    "::after": { content: '""', position: "absolute", backgroundColor: "rgb(255, 255, 255)" },
  });
  const occluded = element({}, {
    innerText: "behind a sibling",
    getBoundingClientRect: () => boxOf(200, 200, 100, 20),
  });
  const overlay = element({});

  const control = element({}, { getBoundingClientRect: () => boxOf(70, 40, 120, 24) });  const label = element({}, {
    textContent: " Name ",
    getBoundingClientRect: () => boxOf(0, 40, 60, 16),
    getAttribute: (name) => (name === "for" ? "field-1" : null),
  });

  // The signature walk, which reads different properties from the same nodes:
  // what each element is, where it sits, and its own words when it has no
  // children to carry them. A leaf, a parent, and one element with no area —
  // the last because a signature that included collapsed nodes would report a
  // difference between two screens that look the same.
  const named = (tag, testid, className, box, own) => element({}, {
    tagName: tag,
    childElementCount: 0,
    attributes: [
      ...(testid ? [{ name: "data-testid", value: testid }] : []),
      ...(className ? [{ name: "class", value: className }] : []),
      // Computed geometry, which the signature must drop: React Flow writes a
      // node's position into `style` on every layout, so keeping it would put
      // the drift straight back that leaving boxes out took away.
      { name: "style", value: "transform: translate(13px, 760px)" },
    ],
    getBoundingClientRect: () => box,
    getAttribute: (attribute) => ({ "data-testid": testid, class: className })[attribute] ?? null,
    ...own,
  });
  const leaf = named("BUTTON", "draft-save", "btn", boxOf(10, 20, 80, 24), { textContent: " Save " });
  const parent = named("DIV", null, "draft-bar", boxOf(0, 0, 760, 44), { childElementCount: 1, textContent: "Save" });
  const arealess = named("SPAN", null, "caret", boxOf(0, 0, 0, 0), { getClientRects: () => [{ width: 0, height: 0 }] });
  // A form control carries its state in properties rather than in the text, so
  // the walk has to read them or two forms holding different things are one
  // screen. The disclosure carries its state in an attribute for the same
  // reason: a `<details>` keeps its subtree mounted either way.
  const typed = named("INPUT", "create-name", "field", boxOf(0, 60, 200, 28), { value: "release-pipeline", checked: false });
  const ticked = named("INPUT", "limit-on", "toggle", boxOf(0, 100, 16, 16), { value: "on", checked: true });
  const disclosure = named("DETAILS", null, "relation-section", boxOf(0, 140, 760, 300), {
    attributes: [{ name: "class", value: "relation-section" }, { name: "open", value: "" }],
    childElementCount: 2,
  });
  // Copy that quotes the harness's own address. The renderer-error fallback
  // names the module it could not fetch, and `serve.mjs` binds an ephemeral
  // port per worker — so without folding the origin this element alone made the
  // state sign differently on every run and every worker, for a reason that
  // says nothing about the product.
  const quoting = named("P", null, "fallback-error", boxOf(0, 460, 760, 20), {
    textContent: " TypeError: Failed to fetch dynamically imported module: http://127.0.0.1:40091/app.mjs ",
  });

  // A React Flow surface, as the settle rule reads it: the count of edges the
  // graph was handed, and the paths actually laid out. One of the two has an
  // element in the document and no geometry yet, which is exactly the state
  // React Flow leaves an edge in between putting it there and computing its
  // path — so counting elements answers "drawn" about a graph that has drawn
  // nothing.
  //
  // Length rather than a box, because the box cannot tell them apart here: an
  // edge between two vertically aligned handles is a straight vertical line and
  // has no width, which is the ordinary shape of a pipeline laid out in a
  // column. `zeroLength` is the pending one — React Flow's path before layout
  // runs both ends from the same point — and `sideways` is a drawn edge whose
  // rect would have failed the old test.
  //
  // `hidden` and `unstroked` are the two that made length *insufficient*. Both
  // have perfect geometry and neither puts a pixel on screen: one line of
  // `.react-flow__edge-path { display: none }` took every edge off every graph
  // in the matrix and each still reported drawn equal to declared. They are in
  // the graph rather than in a test of their own so that the count below is the
  // assertion — five paths in the document, two of them drawn.
  const laidOut = element({}, { getTotalLength: () => 128 });
  const sideways = element({}, { getTotalLength: () => 64, getClientRects: () => [{ width: 90, height: 0 }] });
  const zeroLength = element({}, { getTotalLength: () => 0 });
  const hidden = element({ display: "none" }, { getTotalLength: () => 128 });
  const unstroked = element({ stroke: "none" }, { getTotalLength: () => 128 });
  const graph = element({}, {
    getAttribute: (name) => ({ "data-graph-edges": "2", class: "relation-flow" })[name] ?? null,
    querySelectorAll: () => [laidOut, sideways, zeroLength, hidden, unstroked],
  });

  const matches = {
    ".a": [shown, unpainted, transparent, behindTransparent, flattened, wrapped],
    ".b": [inside, past, collapsed, underLid],
    ".c": [wording, wordless],
    ".d": [ghost],
    ".alpha": [faint],
    ".font": [flat],
    ".clip": [cutAway],
    ".indent": [farAway],
    ".cover": [covered],
    ".occluded": [occluded],
    "label[for]": [label],
    "[data-graph-edges]": [graph],
    "body *": [leaf, parent, arealess, typed, ticked, disclosure, quoting],
  };
  return {
    defaultView: { getComputedStyle: (node, part) => (part ? pseudos.get(node)?.[part] ?? { content: "none" } : styles.get(node) ?? BASE_STYLE) },
    documentElement: { clientWidth: 1280, clientHeight: 900, scrollWidth: 1280 },
    body: { innerText: "Bureau" },
    querySelectorAll: (selector) => matches[selector] ?? [],
    getElementById: (id) => (id === "field-1" ? control : null),
    elementFromPoint: (x, y) => (x === 250 && y === 210 ? overlay : null),
  };
}

/**
 * Both contrast selectors, and the ratio itself.
 *
 * This asserted `CONTRAST.includes(".kind-label")` and that every entry began
 * with a dot. `.access` could be deleted outright and the threshold dropped
 * from 4.5 to 2, and it still passed — so the check that exists because a hue
 * retune is invisible in a screenshot was itself invisible to a hue retune.
 *
 * The set is pinned exactly, and the floor is pinned at its boundary: 4.49 is
 * reported and 4.5 is not. Asserting only that some low ratio is caught would
 * survive any threshold at all above it.
 */
test("both contrast selectors are held to WCAG AA, at the boundary", () => {
  const state = { expect: { shows: [], hides: [], copy: [] } };
  const snapshot = (contrast) => ({ counts: {}, text: "", viewport: { width: 1280, height: 900 }, overflowX: 0, contrast, boxes: [] });
  const at = (selector, ratio) => ({ selector, text: "label", ratio });
  const kinds = (entries) => verdict(state, snapshot(entries)).filter((item) => item.kind === "low-contrast").length;

  assert.deepStrictEqual(
    {
      selectors: [...CONTRAST].sort(),
      below: kinds([at(".kind-label", 4.49), at(".access", 4.49)]),
      atFloor: kinds([at(".kind-label", 4.5), at(".access", 4.5)]),
    },
    { selectors: [".access", ".kind-label"], below: 2, atFloor: 0 },
  );
});

/**
 * One page gets one route, which `interceptOp` assumes by taking the first of
 * whatever `interceptFor` returns. Five axes can now ask for one, and two
 * asking for different routes would be silent and expensive: `draft: saving`
 * (stall) crossed with `run: refused` (fail) would install the stall, the
 * cancel would never be answered, and the state would spend its whole timeout
 * waiting for an error the harness had arranged never to produce.
 *
 * The invariant holds by construction today — the surface and orthogonality
 * rules keep those axes apart — which is exactly why it needs asserting: a
 * later rule relaxation would drop it without a word.
 */
test("no state asks for two different request routes at once", () => {
  const conflicted = STATES
    .filter((state) => state.dimensions)
    .map((state) => ({ id: state.id, routes: interceptFor(state.dimensions) }))
    .filter((entry) => entry.routes.length > 1);

  assert.deepStrictEqual(conflicted, []);
});

/**
 * The route a state rides is written twice — on the page op, which the walkers
 * read to install it, and on the state, which every consumer that asks *about*
 * a state reads instead: the `intercepted` root category, the lab's route tag,
 * the lab's refusal to draw a condition it cannot install, and the suite's list
 * of states the lab can drive.
 *
 * Comparing those two to each other is what this test used to do, and it could
 * not fail. `registry.mjs` derives `state.intercept` as
 * `ops.find((op) => op.intercept)?.intercept`, and the test recomputed that
 * expression character for character; `probes.mjs` writes both from one
 * parameter. Both sides were one read of one value, so `disagreeing` was `[]`
 * by construction. Rerouting every `stall-intent` state to `fail-intent` — an
 * inversion that turns a held save into a refused one on some twenty screens —
 * left the whole file green.
 *
 * So each family is held to a source that is genuinely not the ops:
 *
 * - matrix states are held to `interceptFor(dimensions)`, the function that
 *   *decides* the route, which `interceptOp` then places. A boot state is not
 *   one of these: its route comes from `bootOps` and `interceptFor` knows
 *   nothing about it, so re-deriving it here would restore the tautology.
 * - boot states and routed probes are held to literal id→route maps, which are
 *   data rather than a second copy of the derivation. A probe that stops
 *   writing the route onto its state, or acquires one nobody reviewed, fails
 *   by name.
 */
const BOOT_ROUTES = {
  "surface:boot+data:loading": "stall-state",
  "surface:boot+data:render-error": "block-renderer",
  "surface:boot-editor+data:loading": "stall-state",
  "surface:boot-editor+data:render-error": "block-editor-renderer",
};

const PROBE_ROUTES = {
  "probe--create-saving": "stall-intent",
  "probe--create-refusal-dismissed": "fail-intent",
  "probe--delete-refusal-dismissed": "fail-intent",
  "probe--delete-preflight-refused": "refuse-preflight",
  "probe--delete-preflight-checking": "stall-preflight",
  "probe--repos-add-registering": "stall-intent",
  "probe--repos-add-refused": "fail-intent",
  "probe--reconcile-now-reported": "pass-intent",
  "probe--reconcile-now-started-a-run": "pass-starts-run",
  "probe--replay-opened-from-a-pass": "pass-starts-run",
  "probe--live-count-loading": "stall-runs",
  "probe--draft-save-transport-lost": "abort-intent",
  "probe--editor-save-transport-lost": "abort-intent",
  "probe--run-activity-idle": "empty-runs",
  "probe--reconcile-now-running": "stall-intent",
  "probe--reconcile-now-refused": "fail-intent",
  "probe--run-refusal-dismissed": "fail-intent",
  "probe--run-under-failed-listing": "fail-runs-later",
  "probe--run-activity-unavailable": "fail-runs",
};

/** The id→route map a family of states actually holds, for one comparison. */
function routesOf(states) {
  return Object.fromEntries(states.filter((state) => state.intercept).map((state) => [state.id, state.intercept]));
}

test("a state rides the route its own source decided, not merely the one its ops carry", () => {
  const matrix = STATES.filter((state) => state.kind === "matrix" && !state.surface.startsWith("boot"));
  const misrouted = matrix
    .map((state) => ({ id: state.id, state: state.intercept ?? null, decided: interceptFor(state.dimensions)[0] ?? null }))
    .filter((entry) => entry.state !== entry.decided);
  // Placement, which is the half a consumer cannot see and the half that can
  // actually be wrong. Comparing the op's route to `state.intercept` was a
  // tautology in the same shape as the one above: `registry.mjs` *defines*
  // `state.intercept` as `ops.find((op) => op.intercept)?.intercept`, and
  // `probes.mjs` writes both from one parameter, so the two sides were one
  // read of one value. What the route needs is not to exist somewhere in the
  // ops but to be installed before the page it conditions is fetched — a
  // route hung on any later op leaves the load unrouted and the condition
  // never happens.
  const unrouted = STATES
    .filter((state) => state.intercept)
    .map((state) => ({ id: state.id, carrier: state.ops.findIndex((op) => op.intercept), first: state.ops[0]?.op ?? null }))
    .filter((entry) => entry.carrier !== 0 || entry.first !== "page");

  assert.deepStrictEqual(
    {
      misrouted,
      unrouted,
      boot: routesOf(STATES.filter((state) => state.surface.startsWith("boot"))),
      probes: routesOf(STATES.filter((state) => state.kind === "probe")),
      routed: STATES.filter((state) => state.intercept).length,
    },
    { misrouted: [], unrouted: [], boot: BOOT_ROUTES, probes: PROBE_ROUTES, routed: 106 },
  );
});

/**
 * Exactly one state in the matrix is allowed to be in motion, and the whole
 * value of saying so is that it is one rather than a mood.
 *
 * `settled` was filed for every render for several rounds and asserted for
 * none: the gallery folded the unsettled ones into a note, so a screenshot that
 * had quietly become nondeterministic read exactly like the two that are
 * supposed to move. The registry answers the question now, and this pins the
 * answer — a second state acquiring the exemption is how "deterministic
 * screenshots" would erode one value at a time.
 */
test("one state declares itself in motion, and it is the one whose transport is playing", () => {
  const moving = STATES.filter((state) => state.expect.settles === false).map((state) => state.id);
  assert.deepStrictEqual(moving, ["surface:pipeline+data:validated+mode:replay+run:finished+transport:playing"]);
});

/**
 * The margin that makes `settles: false` an assertion rather than a flake.
 *
 * The claim is "this render never stops changing inside the settle window", and
 * it holds only while playing the run to its end takes materially longer than
 * that window — at the end `useReplayOverlay` clears `playing`, the signature
 * comes to rest, and the state would report settled. Every number is read from
 * whatever owns it: the span from the committed log, the tick from the literal
 * in `replay.js` (browser-only, so it is read rather than imported), the budget
 * from `checks.mjs`. Shorten the log, speed the transport up or lengthen the
 * budget and this fails here, offline, instead of intermittently in CI.
 */
test("playing the finished run to its end takes far longer than the settle budget", async () => {
  const log = await readFile(new URL(`./fixtures/runs/${RUN_IDS.finished}/events.jsonl`, import.meta.url), "utf8");
  const stamps = log.trim().split("\n").map((line) => JSON.parse(line).at_ms);
  const source = await readFile(new URL("../web/replay/replay.js", import.meta.url), "utf8");
  const tick = Number(source.match(/const TICK_MS = (\d+);/u)?.[1]);
  // At 1x the transport advances TICK_MS of run time per TICK_MS of wall time,
  // so the wall clock it needs is the span itself. The tick is still read, and
  // asserted finite, because a speed-up is exactly what would close the margin.
  const wallMs = (stamps.at(-1) - stamps[0]) / (tick * 1) * tick;
  assert.ok(
    Number.isFinite(tick) && wallMs >= SETTLE_BUDGET_MS * 2,
    `playing spans ${wallMs}ms at a ${tick}ms tick, against a ${SETTLE_BUDGET_MS}ms budget`,
  );
});

/**
 * The overlap licence is per selector and per state, not a flag.
 *
 * Bringing the editor's and the relation graph's cards under the overlap rule
 * found a real collision on the one state that drags a card by hand, which is
 * the feature working rather than a layout that computed a collision. The risk
 * in answering that with an allowance is that it becomes a blanket amnesty, so
 * this holds it to the two things that keep it narrow: it drops the region it
 * names, and it keeps every other overlap on the same render.
 */
test("a declared overlap is dropped and an undeclared one on the same render is not", () => {
  const found = [
    { kind: "overlap", detail: ".editor-card #0 overlaps #1" },
    { kind: "overlap", detail: ".assignment-card #0 overlaps #1" },
  ];
  const detailsOf = (list) => list.map((item) => item.detail);
  assert.deepStrictEqual(
    {
      declared: detailsOf(permitted({ expect: { allowOverlap: [".editor-card"] } }, found)),
      undeclared: detailsOf(permitted({ expect: { allowOverlap: [] } }, found)),
    },
    { declared: [".assignment-card #0 overlaps #1"], undeclared: detailsOf(found) },
  );
});
