import { requireValue } from "./maintenance-contract.mjs";
import { MiB, admit } from "./maintenance-resources.mjs";

const BOOTSTRAP = `try {
  await (await import(process.argv[1])).admittedCommand(JSON.parse(process.argv[2]));
} catch (error) {
  console.error("maintenance command admission/exec failed:", error.message);
  process.exitCode = 1;
}`;

export function waitingBounds(bounds) {
  return { ...bounds, maxRss: 0, memoryFloor: 256 * MiB };
}

export function lockedCommand(lockPath, args, context, bounds, timeoutMs) {
  return ["--wait", String(timeoutMs / 1000), "--conflict-exit-code", "75", lockPath,
    process.execPath, "--input-type=module", "-e", BOOTSTRAP, import.meta.url,
    JSON.stringify({ args, context, bounds })];
}

export async function admittedCommand({ args, context, bounds }, {
  check = admit, replace = process.execve, environment = process.env,
} = {}) {
  requireValue(typeof replace === "function", "Linux Node 24 process.execve is required");
  await check(context, bounds);
  // The outer flock process retains its descriptor while this process becomes
  // unshare. No intermediate Node parent can exit and release an active check.
  replace("/bin/sh", ["sh", "-c", 'exec unshare "$@"', "maintenance", ...args], environment);
  throw new Error("maintenance command exec returned without replacing the process");
}
