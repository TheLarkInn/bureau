import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

import { evidence, requireValue, seedFor, validateFindings } from "./maintenance-contract.mjs";
import { BOUNDS, GiB, admit, directoryBytes } from "./maintenance-resources.mjs";
import { boundedChild } from "./maintenance-child.mjs";
import { TOOL_PACKAGES, linkPreparedTools, requireReadOnlyTools, unlinkPreparedTools } from "./maintenance-tools.mjs";

export const CHAOS_TEST = "seeded_offline_invariants";

export class CheckFailure extends Error {
  constructor(message, log) {
    super(message);
    this.log = log;
  }
}

export function git(args, cwd = process.cwd()) {
  return execFileSync("git", ["--no-pager", "--no-optional-locks", ...args], {
    cwd, encoding: "utf8", timeout: 10_000, maxBuffer: 256 * 1024,
    env: { PATH: process.env.PATH, HOME: process.env.HOME, GIT_CONFIG_NOSYSTEM: "1" },
  }).trim();
}

export function workspace(cwd = process.cwd()) {
  return { commit: git(["rev-parse", "HEAD"], cwd), status: git(["status", "--porcelain"], cwd) };
}

export function requireClean(source, cwd = process.cwd()) {
  const state = workspace(cwd);
  requireValue(state.commit === source.commit && state.status === "",
    "reporting changed the source worktree");
}

export function siteResult(run, category) {
  requireValue(!run.problem && !run.signal, run.problem ?? "site checker was signaled");
  const result = JSON.parse(run.stdout);
  const kind = category.replace("site-", "");
  requireValue(result.schema === "bureau-site-check-v1" && result.kind === kind
    && result.complete === true, "site checker returned an incomplete or wrong-kind report");
  requireValue(Number.isSafeInteger(result.checks) && result.checks > 0, "site checker executed no checks");
  validateFindings(result.findings, category);
  requireValue(run.code === (result.findings.length ? 1 : 0),
    "site exit status disagrees with evidence; missing tooling is not a finding");
  return { checks: result.checks, findings: result.findings };
}

export function chaosResult(run, seed) {
  requireValue(!run.problem && !run.signal, run.problem ?? "chaos checker was signaled");
  const summary = /^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;/mu.exec(run.stdout);
  requireValue(summary && Number(summary[2]) + Number(summary[3]) === 1
    && summary.slice(4).every((value) => value === "0"), "chaos test was missing, skipped, or incomplete");
  const failed = summary[1] === "FAILED";
  requireValue(failed ? run.code === 101 && summary[3] === "1" : run.code === 0 && summary[2] === "1",
    "chaos exit status disagrees with the exact test result");
  return { checks: 1, findings: failed ? [{
    id: "seeded-offline-invariants", title: "Seeded offline reconciliation invariant failed",
    path: "crates/bureau/tests/maintenance_chaos.rs",
    detail: `BUREAU_CHAOS_SEED=${seed}; ${run.stdout.slice(-3500)}`,
  }] : [] };
}

async function checkDirectory(root, policy) {
  await admit({ cwd: root, backingPaths: policy.backing_paths, extraPaths: [policy.cargo_target] });
  await directoryBytes(policy.cargo_target, policy.cargo_cache_max_bytes);
  const parent = join(root, "target", "bureau-maintenance");
  await mkdir(parent, { recursive: true });
  return mkdtemp(join(parent, "check-"));
}

function checkOptions(root, scratch, policy) {
  return {
    cwd: root, scratch, backingPaths: policy.backing_paths,
    extraPaths: [policy.cargo_target],
    lockPath: join(dirname(policy.cargo_target), "command.lock"),
    watchedPaths: [{ path: policy.cargo_target, maximum: policy.cargo_cache_max_bytes }],
    environment: { TMPDIR: scratch, CARGO_TARGET_DIR: policy.cargo_target,
      CARGO_BUILD_JOBS: "1", CARGO_INCREMENTAL: "0", RUST_BACKTRACE: "0" },
  };
}

export async function runCheck(source, policy, {
  root = process.cwd(), gates = false, seed = seedFor(source),
} = {}) {
  requireValue(Number.isInteger(seed) && seed >= 0 && seed <= 0xffff_ffff, "check seed must be a u32");
  const scratch = await checkDirectory(root, policy);
  const options = checkOptions(root, scratch, policy);
  let run;
  let links = [];
  let failure;
  try {
    if (gates) {
      for (const path of TOOL_PACKAGES) git(["check-ignore", "--quiet", `${path}/node_modules`], root);
      links = await linkPreparedTools(root, policy);
      const command = [
        "set -euo pipefail",
        "cargo build --offline --quiet --bin bureau",
        "cargo fmt --all -- --check",
        'bash scripts/lint.sh > "$TMPDIR/lint.log" 2>&1 || { tail -c 65536 "$TMPDIR/lint.log"; exit 1; }',
        'cargo test --offline --quiet -- --test-threads=1 > "$TMPDIR/test.log" 2>&1 || { tail -c 65536 "$TMPDIR/test.log"; exit 1; }',
        "printf '%s\\n' 'cargo fmt, scripts/lint.sh and cargo test --offline passed'",
      ].join("\n");
      run = await boundedChild("bash", ["-c", command], {
        ...options, timeoutMs: 900_000, bounds: { ...BOUNDS, maxRss: 4 * GiB },
        environment: { ...options.environment,
          BUREAU_CANVAS_BUREAU: join(policy.cargo_target, "debug", "bureau"),
          BUREAU_SITE_TOOLS: policy.site_tools, PLAYWRIGHT_BROWSERS_PATH: policy.browser_path,
          BUREAU_CHAOS_SEED: String(seed) },
      });
      requireValue(!run.problem && run.code === 0 && !run.signal,
        run.problem ?? `repository gates failed: ${(run.stdout + run.stderr).slice(-4000)}`);
      return { evidence: null, log: run.stdout + run.stderr };
    }
    if (source.category === "chaos") {
      run = await boundedChild("cargo", ["test", "--offline", "--test", "maintenance_chaos", "--",
        CHAOS_TEST, "--exact", "--nocapture", "--test-threads=1"], {
        ...options, timeoutMs: 300_000,
        environment: { ...options.environment, BUREAU_CHAOS_SEED: String(seed) },
      });
    } else {
      await requireReadOnlyTools(policy);
      run = await boundedChild(process.execPath, ["--max-old-space-size=512", "site/check.mjs",
        "--kind", source.category.replace("site-", ""), "--json"], {
        ...options, timeoutMs: 150_000,
        environment: { ...options.environment, BUREAU_SITE_TOOLS: policy.site_tools,
          PLAYWRIGHT_BROWSERS_PATH: policy.browser_path },
      });
    }
    const result = source.category === "chaos" ? chaosResult(run, seed) : siteResult(run, source.category);
    return { evidence: evidence(source, result.checks, result.findings, seed), log: run.stdout + run.stderr };
  } catch (error) {
    failure = error;
    if (run) throw new CheckFailure(error.message, run.stdout + run.stderr);
    throw error;
  } finally {
    try {
      await unlinkPreparedTools(links);
      requireValue(await realpath(scratch) === resolve(scratch), "scratch identity changed; preserve for inspection");
      await rm(scratch, { recursive: true });
    } catch (error) {
      const reason = `${failure ? `${failure.message}; ` : ""}scratch cleanup failed: ${error.message}`;
      throw new CheckFailure(reason, run ? run.stdout + run.stderr : "");
    }
  }
}

export function patchProblem(paths, category) {
  if (!paths.length || paths.length > 20) return "patch must change between one and twenty files";
  for (const path of paths) {
    if (path.split("/").some((part) => ["", ".", ".."].includes(part))) return "patch contains a noncanonical path";
    const protectedPath = /^crates\/bureau\/tests\/maintenance_chaos(?:[/.]|$)/u.test(path)
      || ["crates/bureau/tests/runlog_framing.rs", "crates/bureau/tests/edge/testdir.rs",
        "crates/bureau/src/cli/run/tests.rs", "crates/bureau/src/cli/run/claim/tests.rs",
        "crates/bureau/src/cli/run/observe/tests.rs"].includes(path);
    if (protectedPath) return `patch changes protected verification code: ${path}`;
    const allowed = category.startsWith("site-") ? path.startsWith("site/src/")
      : path.startsWith("crates/") || path === "dylint.toml";
    if (!allowed) return `patch is outside the category scope: ${path}`;
  }
  return null;
}

export function requirePatch(source, root = process.cwd()) {
  const changed = git(["diff", "--no-ext-diff", "--name-only", source.commit, "--"], root);
  const untracked = git(["ls-files", "--others", "--exclude-standard"], root);
  const paths = [...new Set([changed, untracked].join("\n").split("\n").filter(Boolean))];
  const problem = patchProblem(paths, source.category);
  requireValue(!problem, problem);
  git(["diff", "--no-ext-diff", "--check", source.commit, "--"], root);
  const patch = git(["diff", "--no-ext-diff", "--numstat", source.commit, "--"], root);
  requireValue(!patch.split("\n").some((line) => line.startsWith("-\t-")),
    "binary patches require human review");
}

export async function saveEvidence(value, log, step, root = process.cwd()) {
  requireValue(/^[a-z-]+$/u.test(step), "invalid evidence step name");
  const directory = join(root, "target", "bureau-maintenance");
  await mkdir(directory, { recursive: true });
  const path = join(directory, `${step}.json`);
  await writeFile(path, `${JSON.stringify(value)}\n`, "utf8");
  const logPath = join(directory, `${step}.log`);
  await writeFile(logPath, log, "utf8");
  return [{ name: `${step}.json`, path }, { name: `${step}.log`, path: logPath }];
}
