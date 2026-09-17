import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function nodeTestFailure(result, allowSkipped = true) {
  if (result.error) return result.error.message || "test process could not complete";
  if (result.status !== 0 || result.signal) {
    return `test process failed (exit ${result.status}, signal ${result.signal ?? "none"})`;
  }
  const output = result.stdout ?? "";
  const complete = [/^ok 1 /mu, /^1\.\.[1-9]\d*$/mu, /^# tests [1-9]\d*$/mu,
    /^# pass \d+$/mu, /^# fail 0$/mu, /^# cancelled 0$/mu,
    /^# skipped \d+$/mu, /^# todo \d+$/mu];
  if (/^not ok /mu.test(output) || !complete.every((pattern) => pattern.test(output))) {
    return "tests failed, did not run, or returned incomplete TAP";
  }
  if (!allowSkipped && /^# (?:skipped|todo) [1-9]\d*$/mu.test(output)) {
    return "tests were skipped or left pending";
  }
  return null;
}

export function runNodeTests(files, {
  label = "Node tests", cwd = process.cwd(), allowSkipped = true,
  timeout = 900_000, maxBuffer = 8 * 1024 * 1024, spawn = spawnSync,
  stdout = process.stdout, stderr = process.stderr,
} = {}) {
  if (!Array.isArray(files) || !files.length
    || !files.every((file) => typeof file === "string" && file.length && !file.startsWith("-"))) {
    throw new Error(`${label}: supply test files, not runner options`);
  }
  const result = spawn(process.execPath,
    ["--test", "--test-concurrency=1", "--test-reporter=tap", ...files],
    { cwd, encoding: "utf8", timeout, maxBuffer });
  stdout.write(result.stdout ?? "");
  stderr.write(result.stderr ?? "");
  const failure = nodeTestFailure(result, allowSkipped);
  if (failure) throw new Error(`${label}: ${failure}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    runNodeTests(process.argv.slice(3), { label: process.argv[2] ?? "Node tests" });
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
