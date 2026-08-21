import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { configView, pipelineView } from "../lib/view.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);
const referenceUrl = new URL("./fixtures/reference-payload.json", import.meta.url);

async function fixture(url) {
  return JSON.parse(await readFile(url, "utf8"));
}

function clone(value) {
  return structuredClone(value);
}

test("committed config projects counts, use refs, and primary repo", async () => {
  const view = configView(await fixture(committedUrl));

  assert.deepEqual(
    {
      dir: view.dir,
      repos: view.repos.length,
      roles: view.roles.length,
      assignments: view.assignments.length,
      pipelines: view.pipelines.length,
      orphans: view.orphans.length,
      primaryRepo: view.assignments[0].primaryRepo,
      terminalLabels: [
        view.assignments[0].work.abortLabel,
        view.assignments[0].work.escalateLabel,
      ],
    },
    {
      dir: ".bureau",
      repos: 1,
      roles: 2,
      assignments: 1,
      pipelines: 1,
      orphans: 0,
      primaryRepo: "bureau",
      terminalLabels: ["bureau:failed", "bureau:needs-human"],
    },
  );

  assert.deepEqual(view.repos[0].usedBy, ["assignment:agent-eligible"]);
  assert.deepEqual(view.pipelines[0].usedBy, ["assignment:agent-eligible"]);
  assert.deepEqual(
    view.roles.map((role) => [role.name, role.usedBy]),
    [
      ["implementer", ["assignment:agent-eligible", "pipeline:agent-eligible-pipeline/implement"]],
      ["reviewer", ["pipeline:agent-eligible-pipeline/review"]],
    ],
  );
});

test("config null yields empty views", () => {
  const payload = { ok: false, errors: [{ path: "repos.yaml", message: "missing" }], config: null };

  assert.deepEqual(configView(payload), {
    dir: "",
    repos: [],
    roles: [],
    assignments: [],
    pipelines: [],
    orphans: [],
  });
  assert.deepEqual(pipelineView(payload, "missing"), {
    name: "missing",
    steps: [],
    terminals: [],
    edges: [],
  });
});

test("absent blocked outcome produces no control edge", async () => {
  const view = pipelineView(await fixture(committedUrl), "agent-eligible-pipeline");

  assert.equal(
    view.edges.some((edge) => edge.source === "verify" && edge.relation === "control" && edge.outcome === "blocked"),
    false,
  );
});

test("reference pipeline projects nine steps, referenced terminals, and retry edges", async () => {
  const view = pipelineView(await fixture(referenceUrl), "fix-failing-test");
  const retrySources = view.edges
    .filter(
      (edge) =>
        edge.relation === "control" &&
        edge.target === "propose" &&
        ["passed", "verdict", "verify"].includes(edge.source),
    )
    .map((edge) => `${edge.source}:${edge.outcome}`)
    .sort();

  assert.deepEqual(
    { steps: view.steps.length, terminals: view.terminals.length, retrySources },
    {
      steps: 9,
      terminals: 2,
      retrySources: ["passed:failure", "verdict:failure", "verify:failure"],
    },
  );
});

test("decision maps produce exactly four control edges", async () => {
  const view = pipelineView(await fixture(referenceUrl), "fix-failing-test");
  const outcomes = view.edges
    .filter((edge) => edge.source === "passed" && edge.relation === "control")
    .map((edge) => edge.outcome)
    .sort();

  assert.deepEqual(outcomes, ["blocked", "failure", "no-work", "success"]);
});

test("concurrent members keep parent id and have no control edges", async () => {
  const view = pipelineView(await fixture(referenceUrl), "fix-failing-test");
  const members = view.steps
    .filter((step) => ["apply", "review"].includes(step.id))
    .map((step) => [step.id, step.parentId])
    .sort();
  const memberControlEdges = view.edges.filter(
    (edge) => ["apply", "review"].includes(edge.source) && edge.relation === "control",
  );

  assert.deepEqual({ members, memberControlEdges }, {
    members: [
      ["apply", "run-checks"],
      ["review", "run-checks"],
    ],
    memberControlEdges: [],
  });
});

test("data and observes relations stay distinct", async () => {
  const view = pipelineView(await fixture(referenceUrl), "fix-failing-test");
  const relationCounts = Object.groupBy(view.edges, (edge) => edge.relation);

  assert.deepEqual(
    {
      control: relationCounts.control?.length ?? 0,
      data: relationCounts.data?.length ?? 0,
      observes: relationCounts.observes?.length ?? 0,
    },
    { control: 25, data: 7, observes: 2 },
  );
});

test("orphaned pipeline appears in config view", async () => {
  const payload = clone(await fixture(committedUrl));
  payload.config.pipelines.unused = { name: "unused", steps: [] };

  assert.deepEqual(configView(payload).orphans, [{ kind: "pipeline", name: "unused" }]);
});