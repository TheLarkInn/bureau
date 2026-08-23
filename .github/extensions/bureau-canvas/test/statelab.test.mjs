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
import { CONCURRENT_STATE } from "../web/statelab/concurrent-state.mjs";
import { buildConcurrentState, PROJECTED_FIELDS } from "./support/concurrent-state.mjs";
import { relationView } from "../lib/view.mjs";
import { DIMENSIONS, valuesOf } from "../web/statelab/dimensions.mjs";
import { collect, CONTRAST, measureFor, verdict } from "../web/statelab/checks.mjs";
import { ADAPTER_VERBS, isAction } from "../web/statelab/driver.mjs";
import { enumerate } from "../web/statelab/enumerate.mjs";
import { applyFixture, FIXTURE_IDS, FIXTURES } from "../web/statelab/fixtures.mjs";
import { SAMPLE_STEP_COUNT, RUN_END, RUN_IDS, RUN_STEP, interceptFor } from "../web/statelab/paths.mjs";
import { EXCLUSIONS, ENTRY_TRANSITIONS, ORDER, REVERSIBLE, STATES, summary, TRANSITIONS } from "../web/statelab/registry.mjs";

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
 * `relationView` derives the graph from the config's own lists: one node per
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

test("every scoping rule is held to account by a crossing probe that really breaks it", () => {  const probes = STATES.filter((state) => state.kind === "probe");
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
 * reaches, so it is the one kind that can quietly cost coverage. Three claims
 * are checked, and the third is the one that cannot be talked around: enumerate
 * again without the rule, and if the kept set does not grow then the rule hides
 * nothing of its own and is claiming a cost it does not impose.
 */
test("every harness rule names its limit, stands on a rendered screen, and really hides one", () => {
  const harness = CONSTRAINTS.filter((rule) => rule.kind === "harness");
  const rendered = new Set(STATES.map((state) => state.id));
  const base = enumerate(ORDER, valuesOf).kept.length;
  const withoutRule = (rule) => enumerate(ORDER, valuesOf, CONSTRAINTS.filter((item) => item.id !== rule.id)).kept.length;

  assert.deepStrictEqual(
    {
      unnamed: harness.filter((rule) => !rule.limit?.trim()).map((rule) => rule.id),
      unstood: harness.filter((rule) => !rendered.has(rule.stands)).map((rule) => rule.id),
      costless: harness.filter((rule) => withoutRule(rule) <= base).map((rule) => rule.id),
      // The obligations belong to the kind. A structural or scoping rule
      // carrying them reads as a harness limit that was never re-kinded.
      mislabelled: CONSTRAINTS.filter((rule) => rule.kind !== "harness" && (rule.limit || rule.stands)).map((rule) => rule.id),
      unknownKind: CONSTRAINTS.filter((rule) => !["structural", "scoping", "harness"].includes(rule.kind)).map((rule) => rule.id),
    },
    { unnamed: [], unstood: [], costless: [], mislabelled: [], unknownKind: [] },
  );
});

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
 * moved really moved. The browser suite then executes it and holds the render
 * to `to`'s own expectations, which is where "opens but never closes" — or
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
 * produces nothing, silently, and the suite walks eleven toggles minus however
 * many were quietly dead. The forge-signals disclosure was exactly that: named
 * in `REVERSIBLE`, asserted nowhere, on the very control this work changed from
 * open-only to a toggle.
 *
 * A toggle is matched by the control that *opened* it rather than by its undo,
 * because two toggles may share one undo — Live and Replay both leave by the
 * Design button. Keyed on the undo, one mode-design return edge answered for
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
 * only the standing regions is how a clipped Save button passed: nothing ever
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
 * `ReferenceError` inside every single `page.evaluate`, so all 440 renders and
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

  assert.deepStrictEqual(rebuilt(doc, { selectors: [".a"], measure: [".b"], contrast: [".c"] }), {
    counts: { ".a": 1 },
    boxes: [
      { selector: ".b", id: "node-0", x: 10, y: 10, width: 100, height: 20, parent: "parent-0", flow: true, clipped: false, trimmed: 0 },
      { selector: ".b", id: "node-1", x: 300, y: 10, width: 50, height: 20, parent: "parent-0", flow: true, clipped: true, trimmed: 150 },
    ],
    contrast: [{ selector: ".c", text: "Kind", ratio: 21 }],
    labels: [{
      text: "Name",
      label: { x: 0, y: 40, width: 60, height: 16 },
      control: { x: 70, y: 40, width: 120, height: 24 },
    }],
    text: "Bureau",
    overflowX: 0,
    viewport: { width: 1280, height: 900 },
  });
});

const BASE_STYLE = {
  visibility: "visible",
  overflowX: "visible",
  overflowY: "visible",
  backgroundColor: "rgb(255, 255, 255)",
  color: "rgb(0, 0, 0)",
  position: "static",
};

function boxOf(x, y, width, height) {
  return { x, y, width, height, top: y, left: x, right: x + width, bottom: y + height };
}

/**
 * A document shaped so that each of `collect`'s inner helpers is called at
 * least once — `visible`, `keyFor`, `idFor`, `clipper`, `channels`, `opaque`,
 * `luminance`, `backdrop` and `rectOf`. Fidelity to a browser is not the
 * point; reaching the lines is.
 */
function pageStub() {
  const styles = new Map();
  const element = (style, own = {}) => {
    const node = {
      getClientRects: () => [{}],
      getBoundingClientRect: () => boxOf(0, 0, 0, 0),
      parentElement: null,
      textContent: "",
      getAttribute: () => null,
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

  // `visible` has to answer both ways over a node that still reports rects.
  const shown = element({});
  const unpainted = element({ visibility: "hidden" });

  // Transparent over white, so `backdrop` must walk up and `opaque` answers
  // false then true before `luminance` runs on what it settles on.
  const painted = element({ backgroundColor: "rgb(255, 255, 255)" });
  const wording = element(
    { backgroundColor: "rgba(0, 0, 0, 0)" },
    { parentElement: painted, textContent: " Kind " },
  );
  const wordless = element({}, { parentElement: painted, textContent: "   " });

  const control = element({}, { getBoundingClientRect: () => boxOf(70, 40, 120, 24) });
  const label = element({}, {
    textContent: " Name ",
    getBoundingClientRect: () => boxOf(0, 40, 60, 16),
    getAttribute: (name) => (name === "for" ? "field-1" : null),
  });

  const matches = {
    ".a": [shown, unpainted],
    ".b": [inside, past, collapsed],
    ".c": [wording, wordless],
    "label[for]": [label],
  };
  return {
    defaultView: { getComputedStyle: (node) => styles.get(node) ?? BASE_STYLE },
    documentElement: { clientWidth: 1280, clientHeight: 900, scrollWidth: 1280 },
    body: { innerText: "Bureau" },
    querySelectorAll: (selector) => matches[selector] ?? [],
    getElementById: (id) => (id === "field-1" ? control : null),
  };
}

test("every contrast selector names small text coloured from a kind hue", () => {
  // A hue that reads as decoration rather than text is a defect the
  // screenshots would not show, so the ratio is asserted rather than eyeballed.
  assert.ok(CONTRAST.includes(".kind-label"));
  assert.deepStrictEqual(CONTRAST.filter((selector) => !selector.startsWith(".")), []);
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
