// The epic's acceptance criterion (#43): build a complete config from an empty
// directory through the actions alone, have `bureau validate` accept it, then
// take it all away again.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { applyPlan, create, crudActions, emptyPlan, remove, rename } from "../lib/crud.mjs";
import { findings } from "../lib/findings.mjs";
import { skipWithoutBureau } from "./support/bureau-binary.mjs";

const needsBureau = await skipWithoutBureau();

const repoRoot = fileURLToPath(new URL("../../../../", import.meta.url));
// The `bureau` binary runs inside WSL, so scratch config must live where it can
// see it. `target/` is gitignored and is not a scanned config directory.
const scratchRoot = fileURLToPath(new URL("../../../../target/canvas-crud-tests/", import.meta.url));

async function withDir(fn) {
  await mkdir(scratchRoot, { recursive: true });
  const dir = await mkdtemp(join(scratchRoot, "crud-"));
  try {
    await Promise.all(["roles", "assignments", "pipelines"].map((sub) => mkdir(join(dir, sub), { recursive: true })));
    await fn(dir);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Holds one plan in memory, exactly as the extension does per instance. */
function session(dir) {
  let plan = emptyPlan();
  const deps = {
    getPlan: () => plan,
    setPlan: (_instance, next) => {
      plan = next;
    },
    loadFindings: (target) => findings(target ?? dir, { cwd: repoRoot }),
  };
  return {
    deps,
    plan: () => plan,
    call: (handler, input) => handler({ instanceId: "test", input: { dir, ...input } }, deps),
    save: () => applyPlan(dir, plan, {}),
    reset: () => {
      plan = emptyPlan();
    },
  };
}

async function buildAll(app) {
  await app.call(create, { kind: "repo", name: "bureau", fields: { url: "https://x/y.git", forge: "github", access: "push", credential: "github-main" } });
  await app.call(create, { kind: "role", name: "implementer", fields: { permissions: ["repo:read", "repo:write", "model:invoke"] } });
  await app.call(create, { kind: "pipeline", name: "build" });
  await app.call(create, {
    kind: "assignment",
    name: "work",
    fields: { work: { forge: "github", source: "a/b", filter: "is:open" }, repos: ["bureau"], pipeline: "build", role: "implementer", verify: "cargo test --offline" },
  });
}

test("builds a whole config from empty and bureau validate accepts it", { skip: needsBureau }, async () => {
  await withDir(async (dir) => {
    const app = session(dir);
    await buildAll(app);
    await app.save();

    const result = await findings(dir, { cwd: repoRoot });

    assert.deepEqual({ state: result.state, ok: result.ok, errors: result.errors }, { state: "validated", ok: true, errors: [] });
  });
});

test("nothing reaches disk until a save", async () => {
  await withDir(async (dir) => {
    const app = session(dir);
    await buildAll(app);

    const before = await readdir(join(dir, "roles"));
    await app.save();
    const after = await readdir(join(dir, "roles"));

    assert.deepEqual({ before, after }, { before: [], after: ["implementer.yaml"] });
  });
});

test("deleting requires confirmation and returns the preflight first", { skip: needsBureau }, async () => {
  await withDir(async (dir) => {
    const app = session(dir);
    await buildAll(app);
    await app.save();
    app.reset();

    const asked = await app.call(remove, { kind: "role", name: "implementer" });
    const pending = app.plan().removals.length;

    assert.deepEqual(
      { confirmed: asked.confirmed, blocking: asked.blocking, referrers: asked.referrers.map((item) => item.kind), pending },
      { confirmed: false, blocking: true, referrers: ["assignment"], pending: 0 },
    );
  });
});

test("tears the whole config back down again", async () => {
  await withDir(async (dir) => {
    const app = session(dir);
    await buildAll(app);
    await app.save();
    app.reset();

    for (const [kind, name] of [["assignment", "work"], ["pipeline", "build"], ["role", "implementer"], ["repo", "bureau"]]) {
      await app.call(remove, { kind, name, confirm: true });
    }
    await app.save();

    const left = await Promise.all(["roles", "assignments", "pipelines"].map((sub) => readdir(join(dir, sub))));

    assert.deepEqual(left, [[], [], []]);
  });
});

test("renaming a role cascades to every referrer and still validates", { skip: needsBureau }, async () => {
  await withDir(async (dir) => {
    const app = session(dir);
    await buildAll(app);
    await app.save();
    app.reset();

    const renamed = await app.call(rename, { kind: "role", from: "implementer", to: "builder" });
    await app.save();
    const result = await findings(dir, { cwd: repoRoot });
    const roles = await readdir(join(dir, "roles"));

    assert.deepEqual(
      { cascaded: renamed.cascaded > 0, roles, state: result.state, errors: result.errors },
      { cascaded: true, roles: ["builder.yaml"], state: "validated", errors: [] },
    );
  });
});

test("declares its verbs and no reserved names", () => {
  assert.deepEqual(
    {
      names: crudActions.map((action) => action.name).sort(),
      reserved: crudActions.some((action) => action.name.startsWith("canvas.")),
      schemas: crudActions.every((action) => Boolean(action.inputSchema) && Boolean(action.handler)),
    },
    { names: ["create", "delete", "rename", "set_limits", "set_repos", "set_role"], reserved: false, schemas: true },
  );
});
