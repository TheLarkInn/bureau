import { spawn } from "node:child_process";

import { BOUNDS, admit, directoryBytes, processGroupUsage, runningProblem } from "./maintenance-resources.mjs";
import { requireValue } from "./maintenance-contract.mjs";

export function childEnvironment(extra = {}) {
  const allowed = ["TMPDIR", "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS", "CARGO_INCREMENTAL",
    "RUST_BACKTRACE", "BUREAU_CHAOS_SEED", "BUREAU_CANVAS_BUREAU",
    "BUREAU_SITE_TOOLS", "PLAYWRIGHT_BROWSERS_PATH"];
  requireValue(Object.keys(extra).every((key) => allowed.includes(key)
    && typeof extra[key] === "string"), "unapproved child environment variable");
  const environment = { PATH: process.env.PATH ?? "/usr/bin:/bin", LANG: "C.UTF-8" };
  if (process.env.HOME) environment.HOME = process.env.HOME;
  return { ...environment, ...extra };
}

export async function boundedChild(command, args, {
  cwd, scratch, timeoutMs = 150_000, backingPaths = [], extraPaths = [],
  environment = {}, bounds = BOUNDS, lockPath, watchedPaths = [],
} = {}) {
  requireValue(Number.isSafeInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 900_000,
    "invalid child deadline");
  await admit({ cwd, backingPaths, extraPaths }, bounds);
  const isolated = [
    "--user", "--map-root-user", "--pid", "--fork", "--mount-proc", "--kill-child=SIGKILL",
    command, ...args,
  ];
  const executable = lockPath ? "flock" : "unshare";
  const argv = lockPath
    ? ["--nonblock", "--conflict-exit-code", "75", lockPath, "unshare", ...isolated] : isolated;
  const child = spawn(executable, argv, {
    cwd, env: childEnvironment(environment), detached: true, stdio: ["ignore", "pipe", "pipe"],
  });
  const output = { stdout: [], stderr: [], bytes: 0 };
  let problem = null;
  let checking = false;
  let spawned = false;
  let finished = false;
  const terminate = (reason) => {
    problem ??= reason;
    if (!spawned || finished) return;
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch (error) {
      if (error.code !== "ESRCH") problem = `process-group cancellation failed: ${error.message}`;
    }
  };
  const cancelled = () => terminate("maintenance child cancelled by signal");
  for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, cancelled);
  const capture = (stream) => (chunk) => {
    output.bytes += chunk.length;
    if (output.bytes > bounds.maxOutput) terminate("combined output ceiling exceeded");
    else output[stream].push(chunk);
  };
  child.stdout.on("data", capture("stdout"));
  child.stderr.on("data", capture("stderr"));
  const timer = setTimeout(() => terminate("maintenance child deadline exceeded"), timeoutMs);
  const monitor = setInterval(async () => {
    if (!spawned || finished || checking) return;
    checking = true;
    try {
      await admit({ cwd, backingPaths, extraPaths }, { ...bounds, memoryFloor: 256 * 1024 * 1024 });
      const usage = await processGroupUsage(child.pid);
      const size = scratch ? await directoryBytes(scratch, bounds.maxScratch) : 0;
      for (const watched of watchedPaths) await directoryBytes(watched.path, watched.maximum);
      const failure = runningProblem({ ...usage, output: output.bytes, scratch: size }, bounds);
      if (failure) terminate(failure);
    } catch (error) {
      terminate(error.message);
    } finally {
      checking = false;
    }
  }, 250);
  try {
    const result = await new Promise((resolveRun) => {
      child.once("spawn", () => { spawned = true; if (problem) terminate(problem); });
      child.once("error", (error) => resolveRun({ code: null, signal: null, error: error.message }));
      child.once("close", (code, signal) => resolveRun({ code, signal }));
    });
    finished = true;
    clearInterval(monitor);
    // Recheck at completion; an unobserved guard is never a successful check.
    if (!problem) {
      try {
        await admit({ cwd, backingPaths, extraPaths }, { ...bounds, memoryFloor: 256 * 1024 * 1024 });
        if (scratch) await directoryBytes(scratch, bounds.maxScratch);
        for (const watched of watchedPaths) await directoryBytes(watched.path, watched.maximum);
      } catch (error) {
        problem = error.message;
      }
    }
    return { ...result, problem: problem ?? result.error ?? null,
      stdout: Buffer.concat(output.stdout).toString("utf8"),
      stderr: Buffer.concat(output.stderr).toString("utf8"), outputBytes: output.bytes };
  } finally {
    clearTimeout(timer);
    clearInterval(monitor);
    for (const signal of ["SIGINT", "SIGTERM"]) process.off(signal, cancelled);
  }
}
