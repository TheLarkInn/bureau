// `set_repos`: the assignment's ordered list, plus an optional registry
// entry, as one plan. Runs against a real temporary config directory, since
// the registry write reads `repos.yaml` from disk.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { crudActions, emptyPlan } from "../lib/crud.mjs";

const ASSIGNMENT = `name: odsp-web-fixes
work:
  forge: ado
  source: EFun/Web
  filter: "[System.AssignedTo] = @Me"
repos:
- odsp-web
- spo.core
pipeline: fix-failing-test
role: implementer
verify: rush test
branch_prefix: bureau/
`;

const REGISTRY = `repos:
  odsp-web:
    url: https://onedrive.visualstudio.com/ODSP-Web/_git/odsp-web
    forge: ado
    access: push
    credential: ado-main
  spo.core:
    url: https://onedrive.visualstudio.com/ODSP-Web/_git/spo.core
    forge: ado
    access: pr
    credential: ado-main
`;

const RUSHSTACK = {
  name: "rushstack",
  url: "https://github.com/microsoft/rushstack.git",
  forge: "github",
  access: "read",
  credential: "github-main",
};

function action(name) {
  return crudActions.find((candidate) => candidate.name === name);
}

async function withConfig(run) {
  const root = await mkdtemp(join(tmpdir(), "setrepos-"));
  const dir = join(root, ".bureau");
  await mkdir(join(dir, "assignments"), { recursive: true });
  await writeFile(join(dir, "repos.yaml"), REGISTRY);
  await writeFile(join(dir, "assignments", "odsp-web-fixes.yaml"), ASSIGNMENT);
  try {
    return await run(dir);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

/** Captures the plan; nothing here applies it, so no file is written. */
async function run(input) {
  return withConfig(async (dir) => {
    const plans = [];
    const deps = {
      getPlan: () => emptyPlan(),
      setPlan: (_id, plan) => plans.push(plan),
      loadFindings: async () => ({
        ok: true,
        state: "validated",
        config: { repos: { "odsp-web": {}, "spo.core": {} }, assignments: { "odsp-web-fixes": {} } },
      }),
    };
    const result = await action("set_repos").handler({ instanceId: "repos", input: { dir, ...input } }, deps);
    return { result, plan: plans.at(-1) };
  });
}

function listedRepos(text) {
  return text.split("\n").filter((line) => line.startsWith("- "));
}

function registryText(plan) {
  return plan.writes.find((write) => write.path.includes("repos.yaml")).text;
}

test("reordering writes only the assignment", async () => {
  const { plan } = await run({ assignment: "odsp-web-fixes", repos: ["spo.core", "odsp-web"] });

  assert.deepEqual(
    { files: plan.writes.length, isAssignment: plan.writes[0].path.includes("odsp-web-fixes") },
    { files: 1, isAssignment: true },
  );
});

test("a reorder puts the new primary first in the file", async () => {
  const { plan } = await run({ assignment: "odsp-web-fixes", repos: ["spo.core", "odsp-web"] });

  assert.deepEqual(listedRepos(plan.writes[0].text), ["- spo.core", "- odsp-web"]);
});

test("registering a new repo writes the registry and the assignment together", async () => {
  const { plan } = await run({
    assignment: "odsp-web-fixes",
    repos: ["odsp-web", "spo.core", "rushstack"],
    register: RUSHSTACK,
  });
  const paths = plan.writes.map((write) => (write.path.includes("repos.yaml") ? "registry" : "assignment"));

  assert.deepEqual(paths, ["registry", "assignment"]);
});

test("the registered entry carries the access and credential chosen for it", async () => {
  const { plan } = await run({ assignment: "odsp-web-fixes", repos: ["odsp-web", "rushstack"], register: RUSHSTACK });

  assert.deepEqual(
    ["rushstack:", "access: read", "credential: github-main", "forge: github"].map((line) => registryText(plan).includes(line)),
    [true, true, true, true],
  );
});

test("registering keeps the repos already in the registry", async () => {
  const { plan } = await run({ assignment: "odsp-web-fixes", repos: ["odsp-web", "rushstack"], register: RUSHSTACK });

  assert.deepEqual(
    ["odsp-web:", "spo.core:"].map((line) => registryText(plan).includes(line)),
    [true, true],
  );
});

test("registering a name the registry already has is refused", async () => {
  const message = await run({
    assignment: "odsp-web-fixes",
    repos: ["odsp-web"],
    register: { ...RUSHSTACK, name: "odsp-web" },
  }).then(() => null, (error) => error.message);

  assert.equal(message.includes("already in the registry"), true);
});

test("removing a repo leaves the others in order", async () => {
  const { plan } = await run({ assignment: "odsp-web-fixes", repos: ["odsp-web"] });

  assert.deepEqual(listedRepos(plan.writes[0].text), ["- odsp-web"]);
});
