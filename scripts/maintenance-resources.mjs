import { readFile, readdir, realpath, lstat } from "node:fs/promises";
import { homedir, release, tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";

import { requireValue } from "./maintenance-contract.mjs";
import { filesystemSnapshot } from "./maintenance-mount.mjs";

export { processGroupUsage } from "./maintenance-process.mjs";

export const MiB = 1024 * 1024;
export const GiB = 1024 * MiB;
export const BOUNDS = Object.freeze({
  diskFloor: 4 * GiB, memoryFloor: GiB, maxRss: 2 * GiB,
  maxProcesses: 96, maxOutput: MiB, maxScratch: 128 * MiB,
  maxCgroupMemory: 8 * GiB, maxCgroupPids: 256, maxCpus: 4,
});

export function admissionProblem(snapshot, bounds = BOUNDS) {
  const requiredMemory = bounds.maxRss + bounds.memoryFloor;
  if (snapshot.platform !== "linux") return "Linux process and filesystem isolation is required";
  if (snapshot.wsl && !snapshot.backingChecked) return "WSL backing-volume capacity was not supplied";
  if (!snapshot.disks?.length) return "filesystem capacity was not observed";
  for (const disk of snapshot.disks) {
    if (!Number.isFinite(disk.free) || disk.free < bounds.diskFloor) {
      return `disk floor violated at ${disk.path}`;
    }
  }
  if (!Number.isFinite(snapshot.availableMemory) || snapshot.availableMemory < requiredMemory) {
    return "host memory headroom is below the admission floor";
  }
  if (!snapshot.groups?.length) return "cgroup v2 resource limits were not observed";
  const memoryLimits = snapshot.groups.map((group) => group.memoryMax).filter(Number.isFinite);
  const pidLimits = snapshot.groups.map((group) => group.pidsMax).filter(Number.isFinite);
  const cpuLimits = snapshot.groups.map((group) => group.cpus).filter(Number.isFinite);
  if (!memoryLimits.length || Math.min(...memoryLimits) > bounds.maxCgroupMemory) return "finite bounded cgroup memory.max is required";
  if (!pidLimits.length || Math.min(...pidLimits) > bounds.maxCgroupPids) return "finite bounded cgroup pids.max is required";
  if (!cpuLimits.length || Math.min(...cpuLimits) > bounds.maxCpus) return "finite bounded cgroup cpu.max is required";
  for (const group of snapshot.groups) {
    if (!Number.isFinite(group.memoryCurrent) || !Number.isFinite(group.pidsCurrent)) return "cgroup usage is unobservable";
    if (group.memoryMax - group.memoryCurrent < requiredMemory) return "cgroup memory headroom is below the floor";
    if (group.pidsMax - group.pidsCurrent < 16) return "cgroup PID headroom is below the floor";
  }
  return null;
}

function limit(text) {
  if (text.trim() === "max") return Infinity;
  const value = Number(text.trim());
  requireValue(Number.isSafeInteger(value) && value >= 0, "malformed cgroup limit");
  return value;
}

async function groupUsage(path) {
  const [memoryMax, memoryCurrent, pidsMax, pidsCurrent, cpu] = await Promise.all(
    ["memory.max", "memory.current", "pids.max", "pids.current", "cpu.max"]
      .map((name) => readFile(join(path, name), "utf8")),
  );
  const [quota, period] = cpu.trim().split(/\s+/u);
  requireValue(Number(period) > 0, "invalid cgroup CPU period");
  return { memoryMax: limit(memoryMax), memoryCurrent: limit(memoryCurrent),
    pidsMax: limit(pidsMax), pidsCurrent: limit(pidsCurrent), cpus: limit(quota) / Number(period) };
}

async function cgroups() {
  const membership = await readFile("/proc/self/cgroup", "utf8");
  const match = /^0::(\/.*)$/mu.exec(membership);
  requireValue(match && !match[1].split("/").includes(".."), "unusable cgroup v2 membership");
  const root = "/sys/fs/cgroup";
  let path = resolve(root, `.${match[1]}`);
  const groups = [];
  while (path !== root) {
    groups.push(await groupUsage(path));
    path = dirname(path);
  }
  // The mount root may be a delegated cgroup, or the unrestricted host root.
  const entries = await readdir(root);
  if (entries.includes("memory.max")) groups.push(await groupUsage(root));
  return groups;
}

export function backingVerified(backingPaths, disks) {
  if (!backingPaths.length) return false;
  const roots = disks.filter((disk) => disk.path === "/");
  const device = (value) => typeof value === "string" && /^\d+$/u.test(value);
  requireValue(roots.length === 1 && device(roots[0].device),
    "root filesystem identity was not observed");
  return backingPaths.every((path) => {
    const matches = disks.filter((disk) => disk.path === path);
    requireValue(matches.length === 1 && device(matches[0].device)
      && typeof matches[0].readOnly === "boolean", "backing filesystem proof is missing or ambiguous");
    return matches[0].device !== roots[0].device && matches[0].readOnly;
  });
}

export async function resourceSnapshot({ cwd = process.cwd(), backingPaths = [], extraPaths = [] } = {}) {
  requireValue(process.platform === "linux", "resource supervision is Linux-only; Windows cannot emulate this gate");
  const paths = [...new Set(await Promise.all(
    ["/", cwd, tmpdir(), homedir(), ...backingPaths, ...extraPaths].map((path) => realpath(path)),
  ))];
  const disks = await Promise.all(paths.map((path) => filesystemSnapshot(path)));
  const memory = /^MemAvailable:\s+(\d+) kB$/mu.exec(await readFile("/proc/meminfo", "utf8"));
  requireValue(memory, "Linux MemAvailable is unobservable");
  const backingChecked = backingVerified(backingPaths, disks);
  return { platform: process.platform, wsl: /microsoft/iu.test(release()),
    backingChecked, disks,
    availableMemory: Number(memory[1]) * 1024, groups: await cgroups() };
}

export async function admit(options = {}, bounds = BOUNDS) {
  const snapshot = await resourceSnapshot(options);
  const problem = admissionProblem(snapshot, bounds);
  requireValue(!problem, problem);
  return snapshot;
}

function directoryKey(info) {
  requireValue(info.isDirectory() && !info.isSymbolicLink(), "owned root must remain a non-symlink directory");
  requireValue(typeof info.dev === "bigint" && info.dev >= 0n
    && typeof info.ino === "bigint" && info.ino >= 0n,
    "directory identity is unobservable");
  return `${info.dev}:${info.ino}`;
}

export async function directoryIdentity(root, inspect = { lstat }) {
  return directoryKey(await inspect.lstat(root, { bigint: true }));
}

function disappeared(error, path, root) {
  return path !== root && ["ENOENT", "ESRCH"].includes(error.code);
}

export async function directoryBytes(root, maximum = BOUNDS.maxScratch,
  inspect = { lstat, readdir }, expectedIdentity) {
  requireValue(Number.isSafeInteger(maximum) && maximum >= 0, "invalid directory byte ceiling");
  const pending = [root];
  let bytes = 0;
  let entries = 0;
  let identity = expectedIdentity;
  while (pending.length) {
    const path = pending.pop();
    entries += 1;
    requireValue(entries <= 10_000, "scratch byte or entry ceiling exceeded");
    let info;
    try {
      info = await inspect.lstat(path, { bigint: true });
    } catch (error) {
      if (disappeared(error, path, root)) continue;
      throw error;
    }
    const size = typeof info.size === "bigint" ? Number(info.size) : info.size;
    requireValue(Number.isSafeInteger(size) && size >= 0, "directory size is unobservable");
    if (path === root) {
      identity ??= directoryKey(info);
      requireValue(directoryKey(info) === identity, "owned directory identity changed");
    }
    bytes += size;
    requireValue(bytes <= maximum, "scratch byte or entry ceiling exceeded");
    if (info.isDirectory() && !info.isSymbolicLink()) {
      let names;
      try {
        names = await inspect.readdir(path);
        const current = await inspect.lstat(path, { bigint: true });
        requireValue(directoryKey(current) === directoryKey(info), "directory changed during enumeration");
      } catch (error) {
        if (disappeared(error, path, root)) continue;
        throw error;
      }
      requireValue(Array.isArray(names) && entries + pending.length + names.length <= 10_000,
        "scratch entry observation ceiling exceeded");
      for (const name of names) {
        requireValue(typeof name === "string" && name && name !== "." && name !== ".."
          && basename(name) === name, "noncanonical directory entry");
        pending.push(join(path, name));
      }
    }
  }
  requireValue(await directoryIdentity(root, inspect) === identity, "owned directory identity changed");
  return bytes;
}

export function runningProblem({ rss, count, output, scratch }, bounds = BOUNDS) {
  if (![rss, count, output, scratch].every(Number.isFinite)) return "runtime usage is unobservable";
  if (rss > bounds.maxRss) return "process-group memory ceiling exceeded";
  if (count > bounds.maxProcesses) return "process-group PID ceiling exceeded";
  if (output > bounds.maxOutput) return "combined output ceiling exceeded";
  if (scratch > bounds.maxScratch) return "scratch byte ceiling exceeded";
  return null;
}
