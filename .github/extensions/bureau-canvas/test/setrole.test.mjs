// `set_role`: the role an assignment names, plus the role file when one is
// being created, as one plan. Runs against a real temporary config directory.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
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
# The role an agent step falls back to.
role: implementer
verify: cargo test --offline
branch_prefix: bureau/
`;

const PATCHER = {
  agent: "/bureau:implementer",
  adapter: "copilot",
  permissions: ["repo:read", "model:invoke"],
  min_trust: "derived",
};

function action(name) {
  return crudActions.find((candidate) => candidate.name === name);
}

async function run(input) {
  const root = await mkdtemp(join(tmpdir(), "setrole-"));
  const dir = join(root, ".bureau");
  await mkdir(join(dir, "assignments"), { recursive: true });
  await mkdir(join(dir, "roles"), { recursive: true });
  await writeFile(join(dir, "assignments", "agent-eligible.yaml"), ASSIGNMENT);
  const plans = [];
  const deps = {
    getPlan: () => emptyPlan(),
    setPlan: (_id, plan) => plans.push(plan),
    loadFindings: async () => ({
      ok: true,
      state: "validated",
      config: { roles: { implementer: {}, reviewer: {} }, assignments: { "agent-eligible": {} } },
    }),
  };
  try {
    const result = await action("set_role").handler({ instanceId: "role", input: { dir, ...input } }, deps);
    return { result, plan: plans.at(-1) };
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

const roleFile = (plan) => plan.writes.find((write) => write.path.includes(`${join("roles", "")}`));
const assignmentFile = (plan) => plan.writes.find((write) => write.path.includes("agent-eligible.yaml"));

test("choosing an existing role writes only the assignment", async () => {
  const { plan } = await run({ assignment: "agent-eligible", role: "reviewer" });

  assert.deepEqual(
    { files: plan.writes.length, hasRole: assignmentFile(plan).text.includes("role: reviewer") },
    { files: 1, hasRole: true },
  );
});

test("choosing a role leaves every other line untouched", async () => {
  const { plan } = await run({ assignment: "agent-eligible", role: "reviewer" });
  const changed = ASSIGNMENT.split("\n")
    .map((line, index) => [line, assignmentFile(plan).text.split("\n")[index]])
    .filter(([left, right]) => left !== right)
    .map(([left]) => left);

  assert.deepEqual(changed, ["role: implementer"]);
});

test("creating a role writes the role file and the assignment together", async () => {
  const { plan } = await run({ assignment: "agent-eligible", role: "patcher", create: PATCHER });
  const kinds = plan.writes.map((write) => (write.path.includes("roles") ? "role" : "assignment"));

  assert.deepEqual(kinds, ["role", "assignment"]);
});

test("the created role carries the grants and trust chosen for it", async () => {
  const { plan } = await run({ assignment: "agent-eligible", role: "patcher", create: PATCHER });
  const text = roleFile(plan).text;

  assert.deepEqual(
    ["name: patcher", "adapter: copilot", "min_trust: derived", "repo:read", "model:invoke"].map((line) => text.includes(line)),
    [true, true, true, true, true],
  );
});

test("a created role is not granted anything it was not given", async () => {
  const { plan } = await run({ assignment: "agent-eligible", role: "patcher", create: PATCHER });
  const text = roleFile(plan).text;

  assert.deepEqual(
    ["repo:write", "repo:push", "pr:merge"].map((permission) => text.includes(permission)),
    [false, false, false],
  );
});

test("creating a role that already exists is refused", async () => {
  const message = await run({ assignment: "agent-eligible", role: "implementer", create: PATCHER })
    .then(() => null, (error) => error.message);

  assert.equal(message.includes("already exists"), true);
});

test("an elevated grant is written when it is explicitly chosen", async () => {
  const { plan } = await run({
    assignment: "agent-eligible",
    role: "pusher",
    create: { ...PATCHER, permissions: ["repo:read", "repo:write", "repo:push", "model:invoke"] },
  });

  assert.equal(roleFile(plan).text.includes("repo:push"), true);
});
