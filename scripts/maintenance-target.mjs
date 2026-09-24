import { execFile } from "node:child_process";
import { constants } from "node:fs";
import { lstat, open, readdir, realpath, rmdir, unlink } from "node:fs/promises";
import { basename, dirname, join, posix } from "node:path";
import { promisify } from "node:util";

import { requireValue } from "./maintenance-contract.mjs";
import { BOUNDS, CeilingExceeded, directoryBytes, directoryIdentity, directoryKey } from "./maintenance-resources.mjs";

const DIRECTORY_FLAGS = constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW;
const BOOTSTRAP = `try {
  const result = await (await import(process.argv[1])).pruneTarget(process.argv[2]);
  process.stdout.write(JSON.stringify(result));
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}`;

const gone = (error) => ["ENOENT", "ESRCH"].includes(error.code);

export function commandLock(policy) {
  return join(dirname(policy.cargo_target), "command.lock");
}

export async function targetIdentity(root, owner = process.getuid()) {
  requireValue(typeof root === "string" && posix.isAbsolute(root) && posix.normalize(root) === root
    && root !== "/" && await realpath(root) === root, "Cargo target must be a canonical owned cache path");
  const info = await lstat(root, { bigint: true });
  const identity = directoryKey(info);
  requireValue(Number.isSafeInteger(owner) && info.uid === BigInt(owner) && (info.mode & 0o022n) === 0n,
    "Cargo target must be a private directory owned by the maintenance account");
  return identity;
}

async function tolerateGone(operation) {
  try {
    return await operation();
  } catch (error) {
    if (gone(error)) return null;
    throw error;
  }
}

async function removeEntry(base, name, device) {
  requireValue(typeof name === "string" && name && name !== "." && name !== ".."
    && basename(name) === name, "noncanonical Cargo target entry");
  const path = join(base, name);
  const info = await tolerateGone(() => lstat(path, { bigint: true }));
  if (!info) return;
  requireValue(info.dev === device, "Cargo target cleanup refuses to cross a filesystem boundary");
  if (!info.isDirectory()) {
    await tolerateGone(() => unlink(path));
    return;
  }
  const child = await tolerateGone(() => open(path, DIRECTORY_FLAGS));
  if (!child) return;
  try {
    requireValue(directoryKey(await child.stat({ bigint: true })) === directoryKey(info),
      "Cargo target directory changed during cleanup");
    await removeChildren(child, device);
  } finally {
    await child.close();
  }
  await tolerateGone(() => rmdir(path));
}

// Every operation resolves through a pinned parent descriptor and never follows
// a final symlink, so replacing a component cannot redirect deletion elsewhere.
async function removeChildren(handle, device) {
  const base = `/proc/self/fd/${handle.fd}`;
  for (const name of await readdir(base)) await removeEntry(base, name, device);
}

async function emptyTarget(root, identity) {
  const handle = await open(root, DIRECTORY_FLAGS);
  try {
    const info = await handle.stat({ bigint: true });
    requireValue(directoryKey(info) === identity, "Cargo target identity changed before cleanup");
    await removeChildren(handle, info.dev);
  } finally {
    await handle.close();
  }
  requireValue(await directoryIdentity(root) === identity, "Cargo target identity changed during cleanup");
}

export async function pruneTarget(root, {
  bytes = BOUNDS.cargoPruneBytes, entries = BOUNDS.cargoPruneEntries, owner = process.getuid(),
} = {}) {
  const identity = await targetIdentity(root, owner);
  try {
    return { pruned: false, bytes: await directoryBytes(root, bytes, undefined, identity, entries) };
  } catch (error) {
    if (!(error instanceof CeilingExceeded)) throw error;
  }
  await emptyTarget(root, identity);
  requireValue((await readdir(root)).length === 0, "Cargo target cleanup left entries behind");
  // An emptied ext4 directory keeps its allocated size, so only emptiness is bounded.
  return { pruned: true, bytes: await directoryBytes(root, Number.MAX_SAFE_INTEGER, undefined, identity, 1) };
}

export async function lockedPrune(root, lockPath, {
  run = promisify(execFile), waitSeconds = 900, environment = { PATH: process.env.PATH ?? "/usr/bin:/bin" },
} = {}) {
  let stdout;
  try {
    ({ stdout } = await run("flock", ["--wait", String(waitSeconds), "--conflict-exit-code", "75", lockPath,
      process.execPath, "--input-type=module", "-e", BOOTSTRAP, import.meta.url, root],
    { env: environment, timeout: (waitSeconds + 600) * 1000, maxBuffer: 64 * 1024 }));
  } catch (error) {
    throw new Error(`Cargo target cleanup failed: ${error.stderr?.trim() || error.message}`, { cause: error });
  }
  const result = JSON.parse(stdout);
  requireValue(typeof result?.pruned === "boolean" && Number.isSafeInteger(result.bytes)
    && result.bytes >= 0, "Cargo target cleanup returned no measurement");
  return result;
}
