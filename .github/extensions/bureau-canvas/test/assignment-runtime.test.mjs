import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { crudActions, emptyPlan } from "../lib/crud.mjs";

const ASSIGNMENT = `name: work
work:
  forge: github
  source: owner/repo
  filter: is:open
  approval_label: ready
repos:
- repo
pipeline: build
branch_prefix: old/
`;

test("assignment edits compose into one draft write", async () => {
  const root = await mkdtemp(join(tmpdir(), "assignment-runtime-"));
  const dir = join(root, ".bureau");
  const path = join(dir, "assignments", "work.yaml");
  await mkdir(join(dir, "assignments"), { recursive: true });
  await writeFile(path, ASSIGNMENT);
  let plan = emptyPlan();
  const runtimeAction = crudActions.find((candidate) => candidate.name === "set_assignment_runtime");
  const limitsAction = crudActions.find((candidate) => candidate.name === "set_limits");
  const workAction = crudActions.find((candidate) => candidate.name === "plan_work_source");
  const deps = {
    getPlan: () => plan,
    setPlan: (_id, next) => { plan = next; },
    loadFindings: async () => ({ config: {} }),
  };
  try {
    await workAction.handler({
      instanceId: "runtime",
      input: {
        dir,
        assignment: "work",
        work: { forge: "ado", source: "Org/Project", filter: "initial", approval_label: null },
      },
    }, deps);
    await runtimeAction.handler({
      instanceId: "runtime",
      input: {
        dir,
        assignment: "work",
        fields: { filter: "is:open label:ready", approval_label: null, branch_prefix: "bureau/" },
      },
    }, deps);
    await limitsAction.handler({
      instanceId: "runtime",
      input: { dir, assignment: "work", limits: { max_concurrent: 2 } },
    }, deps);
    const text = plan.writes[0].text;
    assert.deepEqual(
      {
        oneWrite: plan.writes.length,
        fields: ["forge: ado", "source: Org/Project", "filter: is:open label:ready", "approval_label: null", "branch_prefix: bureau/", "max_concurrent: 2"]
          .map((line) => text.includes(line)),
      },
      { oneWrite: 1, fields: [true, true, true, true, true, true] },
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
