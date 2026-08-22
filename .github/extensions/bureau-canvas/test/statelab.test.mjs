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
import { DIMENSIONS } from "../web/statelab/dimensions.mjs";
import { CONTRAST, verdict } from "../web/statelab/checks.mjs";
import { ADAPTER_VERBS } from "../web/statelab/driver.mjs";
import { applyFixture, FIXTURE_IDS, FIXTURES } from "../web/statelab/fixtures.mjs";
import { SAMPLE_STEP_COUNT } from "../web/statelab/paths.mjs";
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
  const attributed = EXCLUSIONS.reduce((total, entry) => total + entry.count, 0);

  assert.deepStrictEqual(
    {
      balances: attributed + counts.matrixStates === counts.combinations,
      everyRuleAccountedFor: EXCLUSIONS.length === CONSTRAINTS.length,
      everyRemovalHasAnExample: EXCLUSIONS.every((entry) => entry.count === 0 || entry.example),
      noRuleIsDeadWeight: EXCLUSIONS.every((entry) => entry.count > 0),
    },
    { balances: true, everyRuleAccountedFor: true, everyRemovalHasAnExample: true, noRuleIsDeadWeight: true },
  );
});

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

test("every entry path uses only verbs the driver implements", () => {
  const verbs = new Set(["page", "fixture", ...ADAPTER_VERBS.filter((verb) => !["goto", "publish"].includes(verb))]);
  const unknown = STATES.flatMap((state) => state.ops.filter((op) => !verbs.has(op.op)).map((op) => `${state.id}: ${op.op}`));
  assert.deepStrictEqual(unknown, []);
});

test("every entry path loads a page first and publishes before it acts", () => {
  const broken = STATES.filter((state) => {
    const acting = state.ops.filter((op) => !["wait", "waitGone"].includes(op.op));
    const publishAt = acting.findIndex((op) => op.op === "fixture");
    const firstAction = acting.findIndex((op) => !["page", "fixture"].includes(op.op));
    return acting[0]?.op !== "page" || (firstAction !== -1 && publishAt > firstAction);
  });
  assert.deepStrictEqual(broken.map((state) => state.id), []);
});

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

test("every scoping rule is held to account by a crossing probe, and every probe names a real rule", () => {
  const probes = STATES.filter((state) => state.kind === "probe");
  const probed = new Set(probes.map((state) => state.rule));
  assert.deepStrictEqual(
    {
      unchecked: CONSTRAINTS.filter((rule) => rule.kind === "scoping" && !probed.has(rule.id)).map((rule) => rule.id),
      dangling: probes.filter((state) => !CONSTRAINT_IDS.includes(state.rule)).map((state) => `${state.id} -> ${state.rule}`),
    },
    { unchecked: [], dangling: [] },
  );
});

test("every transition carries the delta that walks it", () => {
  const byId = new Map(STATES.map((state) => [state.id, state]));
  const broken = TRANSITIONS.filter((edge) => {
    const parent = byId.get(edge.from).ops.filter((op) => op.op !== "wait");
    const child = byId.get(edge.to).ops.filter((op) => op.op !== "wait");
    const delta = edge.delta.filter((op) => op.op !== "wait");
    // The parent's operations plus the edge's delta must be the child's.
    return JSON.stringify([...parent, ...delta]) !== JSON.stringify(child);
  });
  assert.deepStrictEqual(broken.map((edge) => `${edge.from} -> ${edge.to}`), []);
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
