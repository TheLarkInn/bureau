// `set_limits`: the assignment's kill switch. The committed files write
// disabled limits as explicit `null` and hang comments above the keys they
// explain, so both have to survive an edit.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { crudActions, emptyPlan } from "../lib/crud.mjs";

const ASSIGNMENT = `name: agent-eligible
work:
  forge: github
  source: TheLarkInn/bureau
  filter: is:open label:agent-eligible
repos:
- bureau
pipeline: agent-eligible-pipeline
role: implementer
verify: cargo test --offline
branch_prefix: bureau/
limits:
  max_concurrent: 1
  max_runs_per_hour: 4
  max_runs_per_day: null
  max_open_prs: null
  # Cost accounting is not measured by the copilot adapter yet (issue
  # 18); a daily cost limit would block every run on unmeasurable usage.
  max_cost_per_day_usd: null
  max_run_hours: null
`;

const COMMITTED = {
  max_concurrent: 1,
  max_runs_per_hour: 4,
  max_runs_per_day: null,
  max_open_prs: null,
  max_cost_per_day_usd: null,
  max_run_hours: null,
};

function action(name) {
  return crudActions.find((candidate) => candidate.name === name);
}

async function run(limits) {
  const root = await mkdtemp(join(tmpdir(), "setlimits-"));
  const dir = join(root, ".bureau");
  await mkdir(join(dir, "assignments"), { recursive: true });
  await writeFile(join(dir, "assignments", "agent-eligible.yaml"), ASSIGNMENT);
  const plans = [];
  const deps = {
    getPlan: () => emptyPlan(),
    setPlan: (_id, plan) => plans.push(plan),
    loadFindings: async () => ({ ok: true, state: "validated", config: { assignments: { "agent-eligible": {} } } }),
  };
  try {
    const result = await action("set_limits").handler(
      { instanceId: "limits", input: { dir, assignment: "agent-eligible", limits } },
      deps,
    );
    return { result, text: plans.at(-1).writes[0].text };
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

function changedLines(after) {
  return ASSIGNMENT.split("\n")
    .map((line, index) => [line, after.split("\n")[index]])
    .filter(([left, right]) => left !== right)
    .map(([left, right]) => [left.trim(), right?.trim()]);
}

test("capping one limit moves only that line", async () => {
  const { text } = await run({ ...COMMITTED, max_runs_per_day: 40 });

  assert.deepEqual(changedLines(text), [["max_runs_per_day: null", "max_runs_per_day: 40"]]);
});

test("the comment explaining a null limit survives the edit", async () => {
  const { text } = await run({ ...COMMITTED, max_concurrent: 3 });

  assert.deepEqual(
    ["# Cost accounting is not measured", "# 18); a daily cost limit"].map((line) => text.includes(line)),
    [true, true],
  );
});

test("turning a limit off writes an explicit null rather than dropping the key", async () => {
  const { text } = await run({ ...COMMITTED, max_runs_per_hour: null });

  assert.deepEqual(
    { hasKey: text.includes("max_runs_per_hour: null"), changed: changedLines(text) },
    { hasKey: true, changed: [["max_runs_per_hour: 4", "max_runs_per_hour: null"]] },
  );
});

test("an omitted key is written as null, so the block keeps every limit", async () => {
  const { text } = await run({ max_concurrent: 2 });
  const written = text.split("\n").filter((line) => line.startsWith("  max_")).map((line) => line.trim());

  assert.deepEqual(written, [
    "max_concurrent: 2",
    "max_runs_per_hour: null",
    "max_runs_per_day: null",
    "max_open_prs: null",
    "max_cost_per_day_usd: null",
    "max_run_hours: null",
  ]);
});

test("setting every limit leaves no null behind", async () => {
  const { text } = await run({
    max_concurrent: 2, max_runs_per_hour: 6, max_runs_per_day: 40,
    max_open_prs: 5, max_cost_per_day_usd: 25, max_run_hours: 12,
  });

  assert.equal(text.includes("null"), false);
});

test("an unchanged block rewrites nothing", async () => {
  const { text } = await run({ ...COMMITTED });

  assert.deepEqual(changedLines(text), []);
});

test("the schema refuses a zero, which would block every run", () => {
  const schema = action("set_limits").inputSchema;

  assert.deepEqual(
    Object.values(schema.properties.limits.properties).map((entry) => entry.minimum),
    [1, 1, 1, 1, 1, 1],
  );
});
