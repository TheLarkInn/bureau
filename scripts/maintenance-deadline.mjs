import { performance } from "node:perf_hooks";

import { CATEGORIES, requireValue } from "./maintenance-contract.mjs";

// Deterministic step requests carry no deadline, so these mirror the pinned
// pipelines' `timeout_secs` (a config test keeps them equal). The engine arms
// that deadline immediately before spawning the step, and detect/reproduce
// run right after intake, far inside the one-hour run deadline.
export const STEP_TIMEOUT_SECONDS = Object.freeze({
  chaos: Object.freeze({ detect: 990, reproduce: 990, "validate-patch": 990, "full-gates": 1590 }),
  "site-accessibility": Object.freeze({ detect: 990, reproduce: 990, "validate-patch": 990, "full-gates": 1740 }),
  "site-responsive": Object.freeze({ detect: 990, reproduce: 990, "validate-patch": 990, "full-gates": 1740 }),
});

// Run budgets start when the shared command lock is acquired, never at spawn.
export const CHECK_TIMEOUT_MS = Object.freeze({ chaos: 300_000, site: 150_000, gates: 900_000 });
// Cargo target measurement and, rarely, in-place emptying under the lock.
export const PRUNE_HOLD_MS = 60_000;
// Per hold: process-group exit, completion rechecks and preparation before the next hold.
export const HOLD_SLACK_MS = 15_000;
// Verification, forge rechecks and evidence after the last hold (git/HTTP calls time out at 10 s each).
export const EXIT_RESERVE_MS = 60_000;
// Sizing only: verification and forge reads before the first hold. Runtime waits measure it.
export const START_RESERVE_MS = 60_000;
// Time for flock to report its own conflict before the helper stops waiting on it.
export const LOCK_EXIT_GRACE_MS = 5_000;
export const LOCK_CONFLICT_EXIT = 75;
export const CHILD_DEADLINE = "maintenance child deadline exceeded";

export function lockBusy(waitMs) {
  return `maintenance command lock busy for ${waitMs / 1000} s; the check did not run`;
}

export function checkKind(category, gates = false) {
  if (gates) return "gates";
  return category === "chaos" ? "chaos" : "site";
}

const checkHoldMs = (kind) => CHECK_TIMEOUT_MS[kind] + HOLD_SLACK_MS;

// One prune plus one check: the longest a step keeps other checks waiting.
export function holdMs(kind) {
  return PRUNE_HOLD_MS + HOLD_SLACK_MS + checkHoldMs(kind);
}

// Covers one hold by each other category's check, which is exactly the
// scheduled case of all three categories admitted in the same cycle.
export function requiredStepMs(category, step) {
  const others = CATEGORIES.filter((other) => other !== category)
    .reduce((total, other) => total + holdMs(checkKind(other)), 0);
  return START_RESERVE_MS + holdMs(checkKind(category, step === "full-gates")) + others + EXIT_RESERVE_MS;
}

export function stepDeadline(category, step) {
  const seconds = STEP_TIMEOUT_SECONDS[category]?.[step];
  requireValue(Number.isSafeInteger(seconds), `no maintenance step deadline for ${category}:${step}`);
  return seconds * 1000;
}

// Forge verification steps take no command lock; every category pins the same
// `timeout_secs` for them (a config test keeps them equal).
export const VERIFY_TIMEOUT_SECONDS = Object.freeze({ "verify-draft": 120, "verify-handoff": 120, "verify-clear": 120 });

export function verifyDeadline(step) {
  const seconds = VERIFY_TIMEOUT_SECONDS[step];
  requireValue(Number.isSafeInteger(seconds), `no maintenance verification deadline for ${step}`);
  return seconds * 1000;
}

function lockWaitSeconds(deadline, reservedMs, now) {
  requireValue(Number.isFinite(deadline), "maintenance lock wait requires the step deadline");
  const seconds = Math.floor((deadline - now - reservedMs) / 1000);
  requireValue(seconds >= 0,
    "maintenance step deadline cannot cover the command lock hold; the check did not run");
  return seconds;
}

export function pruneWaitSeconds(deadline, kind, now = performance.now()) {
  return lockWaitSeconds(deadline, PRUNE_HOLD_MS + HOLD_SLACK_MS + checkHoldMs(kind) + EXIT_RESERVE_MS, now);
}

export function checkWaitSeconds(deadline, kind, now = performance.now()) {
  return lockWaitSeconds(deadline, checkHoldMs(kind) + EXIT_RESERVE_MS, now);
}

// Without `lockWaitMs`, one deadline from spawn covers wait plus run (legacy
// callers). With it, the wait is bounded separately and the run deadline is
// armed only after the lock helper reports acquisition.
export function childDeadline({ timeoutMs, lockWaitMs, locked }, terminate, {
  schedule = setTimeout, cancel = clearTimeout,
} = {}) {
  const phased = locked && lockWaitMs !== undefined;
  const waitMs = phased ? lockWaitMs : timeoutMs;
  let acquired = !locked;
  let timer = phased ? schedule(() => terminate(lockBusy(waitMs)), waitMs + LOCK_EXIT_GRACE_MS)
    : schedule(() => terminate(CHILD_DEADLINE), timeoutMs);
  return {
    acquire() {
      if (acquired) return;
      acquired = true;
      if (!phased) return;
      cancel(timer);
      timer = schedule(() => terminate(CHILD_DEADLINE), timeoutMs);
    },
    problem: (code) => (!acquired && code === LOCK_CONFLICT_EXIT ? lockBusy(waitMs) : null),
    stop: () => cancel(timer),
  };
}
