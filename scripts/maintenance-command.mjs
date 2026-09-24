import { closeSync, writeSync } from "node:fs";

import { requireValue } from "./maintenance-contract.mjs";
import { LOCK_CONFLICT_EXIT } from "./maintenance-deadline.mjs";
import { MiB, admit } from "./maintenance-resources.mjs";

export const ACQUIRED_FD = 3;
export const ACQUIRED = "acquired\n";

const BOOTSTRAP = `try {
  await (await import(process.argv[1])).admittedCommand(JSON.parse(process.argv[2]));
} catch (error) {
  console.error("maintenance command admission/exec failed:", error.message);
  process.exitCode = 1;
}`;

export function waitingBounds(bounds) {
  return { ...bounds, maxRss: 0, memoryFloor: 256 * MiB };
}

export function lockedCommand(lockPath, args, context, bounds, waitMs) {
  requireValue(Number.isSafeInteger(waitMs) && waitMs >= 0, "invalid command lock wait");
  return ["--wait", String(waitMs / 1000), "--conflict-exit-code", String(LOCK_CONFLICT_EXIT), lockPath,
    process.execPath, "--input-type=module", "-e", BOOTSTRAP, import.meta.url,
    JSON.stringify({ args, context, bounds })];
}

// Runs only once flock holds the lock; the parent then arms the run deadline.
// Closing the descriptor keeps the executed check from ever reaching it.
export function reportAcquired() {
  writeSync(ACQUIRED_FD, ACQUIRED);
  closeSync(ACQUIRED_FD);
}

export async function admittedCommand({ args, context, bounds }, {
  acquired = reportAcquired, check = admit, replace = process.execve, environment = process.env,
} = {}) {
  requireValue(typeof replace === "function", "Linux Node 24 process.execve is required");
  acquired();
  await check(context, bounds);
  // The outer flock process retains its descriptor while this process becomes
  // unshare. No intermediate Node parent can exit and release an active check.
  replace("/bin/sh", ["sh", "-c", 'exec unshare "$@"', "maintenance", ...args], environment);
  throw new Error("maintenance command exec returned without replacing the process");
}
