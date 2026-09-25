import { performance } from "node:perf_hooks";
import { setTimeout as sleep } from "node:timers/promises";

import { UnobservedEffect, requireValue } from "./maintenance-contract.mjs";
import { observe } from "./maintenance-lifecycle.mjs";

// Unauthenticated forge reads are publicly cacheable (`max-age=60`), so one can
// omit an effect written up to 60 s before it was served. The reporter's writes
// finish before this step spawns, so reads that start 61 s after the first
// observation began reflect them.
export const RECHECK_AFTER_MS = 61_000;
// The one re-observation (a few reads, each bounded at 10 s) plus result output.
export const RECHECK_RESERVE_MS = 30_000;

const CLOCK = Object.freeze({ now: () => performance.now(), wait: (ms) => sleep(ms) });

function unobserved(error) {
  if (!(error instanceof UnobservedEffect)) throw error;
  return error;
}

async function attempt(api, category, policy, verify) {
  return verify(await observe(api, category, policy));
}

// `deadline` is milliseconds since process start, as the engine arms the step
// timeout immediately before spawn. Only an unobserved reporter effect earns
// one full re-observation and re-verification; any other failure, or a second
// failure of any kind, blocks with both messages.
export async function verifyObserved(api, category, policy, verify, { deadline, clock = CLOCK }) {
  requireValue(Number.isFinite(deadline), "forge verification requires the step deadline");
  const started = clock.now();
  let first;
  try {
    return await attempt(api, category, policy, verify);
  } catch (error) {
    first = unobserved(error);
  }
  const at = Math.max(started + RECHECK_AFTER_MS, clock.now());
  requireValue(at + RECHECK_RESERVE_MS <= deadline,
    `${first.message}; the step deadline leaves no time to re-observe the forge`);
  await clock.wait(at - clock.now());
  try {
    return await attempt(api, category, policy, verify);
  } catch (error) {
    throw new Error(`${first.message}; re-observed ${Math.round((at - started) / 1000)} s after the first `
      + `observation: ${error.message}`, { cause: error });
  }
}
