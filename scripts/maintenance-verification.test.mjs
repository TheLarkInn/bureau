import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import { patchProblem, requireVerificationInputs, runCheck } from "./maintenance-checks.mjs";

function git(root, ...args) {
  return execFileSync("git", ["-C", root, "-c", "commit.gpgsign=false",
    "-c", "user.name=Bureau fixture", "-c", "user.email=fixture@example.invalid", ...args], {
    encoding: "utf8", timeout: 10_000, maxBuffer: 64 * 1024,
  }).trim();
}

async function repository(t) {
  const root = await mkdtemp(join(tmpdir(), "bureau-verification-"));
  t.after(() => rm(root, { recursive: true }));
  await mkdir(join(root, "crates", "sample"), { recursive: true });
  await writeFile(join(root, "Cargo.toml"), '[workspace]\nmembers = ["crates/*"]\n');
  await writeFile(join(root, "crates", "sample", "Cargo.toml"), '[package]\nname = "sample"\nversion = "0.1.0"\n');
  await writeFile(join(root, ".gitignore"), ".cargo/\n");
  git(root, "init", "--quiet");
  git(root, "config", "core.autocrlf", "false");
  git(root, "config", "core.fsmonitor", "false");
  git(root, "config", "core.hooksPath", join(root, "no-hooks"));
  git(root, "add", ".");
  git(root, "commit", "--quiet", "-m", "fixture\n\nCo-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>");
  return { root, source: { commit: git(root, "rev-parse", "HEAD"), category: "chaos" } };
}

const REDIRECT = '[package]\nname = "sample"\nversion = "0.1.0"\n'
  + '[[test]]\nname = "maintenance_chaos"\npath = "tests/always-pass.rs"\n';

test("unchanged target inputs permit ordinary implementation changes", async (t) => {
  const { root, source } = await repository(t);
  await writeFile(join(root, "crates", "sample", "implementation.rs"), "pub fn changed() {}\n");
  await requireVerificationInputs(source, root);
  assert.equal(patchProblem(["crates/sample/implementation.rs"], "chaos"), null);
});

test("nested manifest redirection is rejected before admission or any Cargo execution", async (t) => {
  const { root, source } = await repository(t);
  await writeFile(join(root, "crates", "sample", "Cargo.toml"), REDIRECT);
  assert.match(patchProblem(["crates/sample/Cargo.toml"], "chaos"), /protected/u);
  await assert.rejects(requireVerificationInputs(source, root), /protected verification input changed/u);
  await assert.rejects(runCheck(source, {}, { root, gates: true, seed: 0 }),
    /protected verification input changed/u);
});

test("checkpointed and assume-unchanged manifest edits cannot bypass source pinning", async (t) => {
  const { root, source } = await repository(t);
  const path = "crates/sample/Cargo.toml";
  git(root, "update-index", "--assume-unchanged", path);
  await writeFile(join(root, path), REDIRECT);
  await assert.rejects(requireVerificationInputs(source, root), /protected verification input changed/u);
  git(root, "update-index", "--no-assume-unchanged", path);
  git(root, "add", path);
  git(root, "commit", "--quiet", "-m", "checkpoint\n\nCo-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>");
  await assert.rejects(requireVerificationInputs(source, root), /protected verification inputs changed/u);
});

test("ignored Cargo config and newly autodetected build scripts require human review", async (t) => {
  for (const path of [".cargo/config.toml", "crates/sample/build.rs"]) {
    const { root, source } = await repository(t);
    await mkdir(dirname(join(root, path)), { recursive: true });
    await writeFile(join(root, path), "unreviewed input\n");
    await assert.rejects(requireVerificationInputs(source, root), /untracked verification inputs/u);
  }
});

test("manifest, lock and build-input exclusions apply at every depth and to site patches", () => {
  for (const root of ["", "crates/sample/", "crates/sample/deeper/", "site/src/"]) {
    for (const name of ["Cargo.toml", "Cargo.lock", "package.json", "package-lock.json",
      "build.rs", ".cargo/config", "rust-toolchain.toml", "clippy.toml", ".gitattributes"]) {
      assert.notEqual(patchProblem([`${root}${name}`], root.startsWith("site/") ? "site-responsive" : "chaos"), null);
    }
  }
});
