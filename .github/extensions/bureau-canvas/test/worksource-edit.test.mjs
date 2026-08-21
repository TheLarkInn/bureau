// The work-source edit path end to end: a pasted URL becomes `work` fields
// in the assignment file, written through the CST-preserving codec so the
// review diff stays one hunk.

import assert from "node:assert/strict";
import { cp, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { actions } from "../lib/actions.mjs";

const FIXTURE = fileURLToPath(new URL("./fixtures/worksource-edit/.bureau", import.meta.url));
const ADO_BOARD = "https://onedrive.visualstudio.com/EFun/_boards/board/t/Web/Backlog%20items?System.AssignedTo=%40me";

function action(name) {
  return actions.find((candidate) => candidate.name === name);
}

/** Accepts every draft: this suite is about the write path, not validation. */
function validateDraft(draft) {
  return { ok: true, state: "validated", dir: draft.subject.dir, errors: [], findings: [] };
}

async function withConfig(run) {
  const root = await mkdtemp(join(tmpdir(), "worksource-"));
  const dir = join(root, ".bureau");
  await cp(FIXTURE, dir, { recursive: true });
  const drafts = new Map();
  const deps = {
    getSubject: () => ({ dir }),
    getDraft: (id) => drafts.get(id),
    setDraft: (id, draft) => drafts.set(id, draft),
    clearDraft: (id) => drafts.delete(id),
    validateDraft,
    loadFindings: async () => ({ ok: true, state: "validated", dir, errors: [], findings: [], config: {} }),
    loadAdvisories: async () => [],
  };
  try {
    return await run({ dir, deps });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

function assignmentPath(dir) {
  return join(dir, "assignments", "odsp-web-fixes.yaml");
}

async function setAndSave(dir, deps, input) {
  const ctx = { instanceId: "ws", input: { assignment: "odsp-web-fixes", ...input } };
  await action("set_work_source").handler(ctx, deps);
  await action("save").handler({ instanceId: "ws", input: { assignment: "odsp-web-fixes" } }, deps);
  return readFile(assignmentPath(dir), "utf8");
}

test("a pasted board URL writes forge, source, and filter into the assignment", async () => {
  const text = await withConfig(({ dir, deps }) => setAndSave(dir, deps, { url: ADO_BOARD }));

  assert.deepEqual(
    ["forge: ado", "source: EFun/Web", 'filter: "[System.AssignedTo] = @Me"'].map((line) => text.includes(line)),
    [true, true, true],
  );
});

test("the edit keeps every untouched line, including comments", async () => {
  const before = await readFile(join(FIXTURE, "assignments", "odsp-web-fixes.yaml"), "utf8");
  const after = await withConfig(({ dir, deps }) => setAndSave(dir, deps, { url: ADO_BOARD }));
  const changed = before.split("\n")
    .map((line, index) => [line, after.split("\n")[index]])
    .filter(([left, right]) => left !== right)
    .map(([left]) => left.trim());

  assert.deepEqual(changed, ["forge: github", "source: TheLarkInn/bureau", "filter: is:open label:agent-eligible"]);
});

test("explicit fields set the work source without a URL", async () => {
  const text = await withConfig(({ dir, deps }) =>
    setAndSave(dir, deps, { forge: "github", source: "microsoft/rushstack", filter: "is:open label:bug" }));

  assert.deepEqual(
    ["source: microsoft/rushstack", "filter: is:open label:bug"].map((line) => text.includes(line)),
    [true, true],
  );
});

test("a URL this cannot derive is refused and nothing is written", async () => {
  const outcome = await withConfig(async ({ dir, deps }) => {
    const before = await readFile(assignmentPath(dir), "utf8");
    const failure = await action("set_work_source")
      .handler({ instanceId: "ws", input: { assignment: "odsp-web-fixes", url: "https://gitlab.com/o/r/-/issues" } }, deps)
      .then(() => null, (error) => error.message);
    return { failure, unchanged: before === await readFile(assignmentPath(dir), "utf8") };
  });

  assert.deepEqual(
    { refused: outcome.failure.includes("unrecognized host"), unchanged: outcome.unchanged },
    { refused: true, unchanged: true },
  );
});

test("a call with neither a URL nor fields is refused", async () => {
  const message = await withConfig(({ deps }) =>
    action("set_work_source")
      .handler({ instanceId: "ws", input: { assignment: "odsp-web-fixes" } }, deps)
      .then(() => null, (error) => error.message));

  assert.equal(message.includes("pass a `url`"), true);
});
