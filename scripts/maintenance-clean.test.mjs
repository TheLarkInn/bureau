import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import { activationPaths, requireClean } from "./maintenance-checks.mjs";
import { PUBLISHER_AGENT } from "./maintenance-publish.mjs";

const AGENT = "maintenance-reporter";

function git(root, ...args) {
  return execFileSync("git", ["-C", root, "-c", "commit.gpgsign=false",
    "-c", "user.name=Bureau fixture", "-c", "user.email=fixture@example.invalid", ...args], {
    encoding: "utf8", timeout: 10_000, maxBuffer: 64 * 1024,
  }).trim();
}

async function put(root, path, text = "agent\n") {
  await mkdir(dirname(join(root, path)), { recursive: true });
  await writeFile(join(root, path), text);
}

// A linked run worktree, as the engine cuts from its mirror.
async function worktree(t, tracked = []) {
  const base = await mkdtemp(join(tmpdir(), "bureau-clean-"));
  t.after(() => rm(base, { recursive: true }));
  const mirror = join(base, "mirror");
  for (const path of ["README.md", ".github/agents/other.agent.md", ...tracked]) await put(mirror, path);
  git(mirror, "init", "--quiet");
  git(mirror, "config", "core.autocrlf", "false");
  git(mirror, "config", "core.hooksPath", join(base, "no-hooks"));
  git(mirror, "add", ".");
  git(mirror, "commit", "--quiet", "-m", "fixture");
  const root = join(base, "wt");
  git(mirror, "worktree", "add", "--quiet", "-b", "run", root);
  return { root, source: { commit: git(root, "rev-parse", "HEAD"), category: "chaos" } };
}

// The exact layout `bureau-plugin` direct activation leaves in the worktree.
async function activate(root, agent = AGENT) {
  for (const path of activationPaths(agent)) await put(root, path, "pinned agent bytes\n");
}

test("the reporter's own engine activation does not read as a source change", async (t) => {
  const { root, source } = await worktree(t);
  await activate(root);
  requireClean(source, root, { activeAgent: AGENT });
  assert.deepEqual(activationPaths(AGENT),
    [".github/agents/maintenance-reporter.agent.md", ".claude/agents/maintenance-reporter.md"]);
});

const ESCAPES = [
  ["strict mode still rejects activation", async () => {}, {}],
  ["another agent file", async (root) => put(root, ".github/agents/extra.agent.md"), undefined],
  ["another agent's activation", async (root) => activate(root, "maintenance-chaos-fixer"), undefined],
  ["another claude file", async (root) => put(root, ".claude/agents/extra.md"), undefined],
  ["a modified tracked file", async (root) => put(root, ".github/agents/other.agent.md", "edit\n"), undefined],
  ["a staged source file", async (root) => {
    await put(root, "src.rs");
    git(root, "add", "src.rs");
  }, undefined],
  ["an untracked source file", async (root) => put(root, "notes.txt"), undefined],
  ["a symlinked activation path", async (root) => {
    const [, claude] = activationPaths(AGENT);
    await rm(join(root, claude));
    await symlink(join(root, "README.md"), join(root, claude));
  }, undefined],
  ["a moved HEAD", async (root) => git(root, "commit", "--quiet", "--allow-empty", "-m", "moved"), undefined],
];

test("every change outside the exact activation files still fails", async (t) => {
  for (const [name, change, options] of ESCAPES) {
    const { root, source } = await worktree(t);
    await activate(root);
    await change(root);
    assert.throws(() => requireClean(source, root, options ?? { activeAgent: AGENT }),
      /reporting changed the source worktree/u, name);
  }
});

test("activation over a tracked agent file is a source change", async (t) => {
  const tracked = activationPaths(AGENT);
  const { root, source } = await worktree(t, tracked);
  await activate(root);
  assert.throws(() => requireClean(source, root, { activeAgent: AGENT }),
    /reporting changed the source worktree/u);
});

test("the publisher exempts only the configured reporter agent", async () => {
  const role = await readFile(new URL("../.bureau/maintenance/roles/maintenance-reporter.yaml", import.meta.url),
    "utf8");
  const agent = /^agent: agents\/([a-z-]+)\.agent\.md$/mu.exec(role)?.[1];
  for (const invalid of ["", "../x", "a/b", "Agent", null]) {
    assert.throws(() => activationPaths(invalid), /invalid active agent name/u, String(invalid));
  }
  assert.equal(PUBLISHER_AGENT, agent);
});
