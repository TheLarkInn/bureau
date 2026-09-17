import { mkdtemp, realpath, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { requireValue } from "./maintenance-contract.mjs";
import { boundedChild } from "./maintenance-child.mjs";
import { CHAOS_TEST, chaosResult, workspace } from "./maintenance-checks.mjs";
import { BOUNDS, MiB, admit } from "./maintenance-resources.mjs";
import { binaryIdentity, readSuite } from "./maintenance-suite.mjs";

export function nextSeed(seed) {
  return (Math.imul(seed, 1664525) + 1013904223) >>> 0;
}

export function soakOptions({ seed = 0, minutes = 25, maxIterations = 1000 } = {}) {
  requireValue(Number.isInteger(seed) && seed >= 0 && seed <= 0xffff_ffff, "seed must be a u32");
  requireValue(Number.isFinite(minutes) && minutes > 0 && minutes <= 30, "soak duration must be positive and at most 30 minutes");
  requireValue(Number.isInteger(maxIterations) && maxIterations > 0 && maxIterations <= 2000,
    "iteration ceiling must be between one and 2000");
  return { seed, durationMs: minutes * 60_000, maxIterations };
}

export async function runSoak(options, execute, { now = () => performance.now(), sleep = delay } = {}) {
  const started = now();
  const deadline = started + options.durationMs;
  let seed = options.seed;
  let iterations = 0;
  let failure = null;
  while (now() < deadline && iterations < options.maxIterations) {
    const remaining = deadline - now();
    if (remaining < 30_000 && iterations > 0) {
      await sleep(remaining);
      break;
    }
    const iterationStart = now();
    try {
      const result = chaosResult(await execute(seed, Math.max(1, Math.floor(Math.min(30_000, remaining)))), seed);
      iterations += 1;
      if (result.findings.length) {
        failure = { kind: "invariant", seed, iteration: iterations, findings: result.findings };
        break;
      }
    } catch (error) {
      failure = { kind: "infrastructure", seed, iteration: iterations + 1, message: error.message };
      break;
    }
    seed = nextSeed(seed);
    const pause = Math.min(deadline - now(), 2500 - (now() - iterationStart));
    if (pause > 0) await sleep(pause);
  }
  if (!failure && (iterations === 0 || now() < deadline)) {
    failure = { kind: "incomplete", seed, message: "iteration ceiling reached before the requested duration" };
  }
  return { schema: "bureau-chaos-soak-v1", complete: failure === null, initial_seed: options.seed,
    iterations, elapsed_ms: Math.round(now() - started), requested_ms: options.durationMs, failure };
}

async function main() {
  const { values } = parseArgs({ options: {
    suite: { type: "string" }, scratch: { type: "string" }, backing: { type: "string", multiple: true },
    seed: { type: "string", default: "0" }, minutes: { type: "string", default: "25" },
    "max-iterations": { type: "string", default: "1000" },
  } });
  const options = soakOptions({ seed: Number(values.seed), minutes: Number(values.minutes),
    maxIterations: Number(values["max-iterations"]) });
  requireValue(values.suite && values.scratch, "soak requires --suite PATH and --scratch PATH");
  const backingPaths = values.backing ?? [];
  const root = await realpath(values.scratch);
  await admit({ cwd: root, backingPaths });
  const suite = await readSuite(values.suite);
  const state = workspace();
  requireValue(state.commit === suite.source_commit && !state.status, "suite and clean source checkout differ");
  const parent = await mkdtemp(join(root, "bureau-chaos-"));
  try {
    const result = await runSoak(options, async (seed, timeoutMs) => {
      await admit({ cwd: root, backingPaths });
      requireValue(await binaryIdentity(suite.binary) === suite.identity, "suite binary changed before repetition");
      const scratch = await mkdtemp(join(parent, "iteration-"));
      try {
        const run = await boundedChild(suite.binary, [CHAOS_TEST, "--exact", "--nocapture", "--test-threads=1"], {
          cwd: scratch, scratch, timeoutMs, backingPaths,
          lockPath: join(root, "bureau-maintenance-command.lock"),
          bounds: { ...BOUNDS, maxRss: 512 * MiB, maxProcesses: 64 },
          environment: { TMPDIR: scratch, BUREAU_CHAOS_SEED: String(seed) },
        });
        requireValue(await binaryIdentity(suite.binary) === suite.identity, "suite binary changed during repetition");
        return run;
      } finally {
        requireValue(await realpath(scratch) === resolve(scratch), "iteration scratch identity changed");
        await rm(scratch, { recursive: true });
      }
    });
    console.log(JSON.stringify({ ...result, source_commit: suite.source_commit,
      binary_sha256: suite.sha256, test: suite.test }));
    if (!result.complete) process.exitCode = 1;
  } finally {
    requireValue(await realpath(parent) === resolve(parent), "campaign scratch identity changed");
    await rm(parent, { recursive: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main().catch((error) => {
    console.log(JSON.stringify({ schema: "bureau-chaos-soak-v1", complete: false,
      failure: { kind: "admission", message: error.message } }));
    process.exitCode = 1;
  });
}
