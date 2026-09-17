import { readFile, readdir, realpath, statfs, lstat } from "node:fs/promises";
import { homedir, release, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

import { requireValue } from "./maintenance-contract.mjs";

export const MiB = 1024 * 1024;
export const GiB = 1024 * MiB;
export const BOUNDS = Object.freeze({
  diskFloor: 4 * GiB, memoryFloor: GiB, maxRss: 2 * GiB,
  maxProcesses: 96, maxOutput: MiB, maxScratch: 128 * MiB,
  maxCgroupMemory: 8 * GiB, maxCgroupPids: 256, maxCpus: 4,
});

export function admissionProblem(snapshot, bounds = BOUNDS) {
  if (snapshot.platform !== "linux") return "Linux process and filesystem isolation is required";
  if (snapshot.wsl && !snapshot.backingChecked) return "WSL backing-volume capacity was not supplied";
  if (!snapshot.disks?.length) return "filesystem capacity was not observed";
  for (const disk of snapshot.disks) {
    if (!Number.isFinite(disk.free) || disk.free < bounds.diskFloor) {
      return `disk floor violated at ${disk.path}`;
    }
  }
  if (!Number.isFinite(snapshot.availableMemory) || snapshot.availableMemory < bounds.memoryFloor) {
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
    if (group.memoryMax - group.memoryCurrent < bounds.memoryFloor) return "cgroup memory headroom is below the floor";
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

export async function resourceSnapshot({ cwd = process.cwd(), backingPaths = [], extraPaths = [] } = {}) {
  requireValue(process.platform === "linux", "resource supervision is Linux-only; Windows cannot emulate this gate");
  const paths = [...new Set(await Promise.all(
    ["/", cwd, tmpdir(), homedir(), ...backingPaths, ...extraPaths].map((path) => realpath(path)),
  ))];
  const disks = await Promise.all(paths.map(async (path) => {
    const fs = await statfs(path, { bigint: true });
    const bytes = fs.bavail * fs.bsize;
    requireValue(bytes <= BigInt(Number.MAX_SAFE_INTEGER), "filesystem capacity exceeds the safe numeric range");
    return { path, free: Number(bytes) };
  }));
  const memory = /^MemAvailable:\s+(\d+) kB$/mu.exec(await readFile("/proc/meminfo", "utf8"));
  requireValue(memory, "Linux MemAvailable is unobservable");
  return { platform: process.platform, wsl: /microsoft/iu.test(release()),
    backingChecked: backingPaths.length > 0, disks,
    availableMemory: Number(memory[1]) * 1024, groups: await cgroups() };
}

export async function admit(options = {}, bounds = BOUNDS) {
  const snapshot = await resourceSnapshot(options);
  const problem = admissionProblem(snapshot, bounds);
  requireValue(!problem, problem);
  return snapshot;
}

export async function directoryBytes(root, maximum = BOUNDS.maxScratch) {
  const pending = [root];
  let bytes = 0;
  let entries = 0;
  while (pending.length) {
    const path = pending.pop();
    const info = await lstat(path);
    requireValue(!info.isSymbolicLink(), "scratch contains a symlink");
    bytes += info.size;
    entries += 1;
    requireValue(bytes <= maximum && entries <= 10_000, "scratch byte or entry ceiling exceeded");
    if (info.isDirectory()) {
      for (const name of await readdir(path)) pending.push(join(path, name));
    }
  }
  return bytes;
}

export async function processGroupUsage(pgid) {
  let rss = 0;
  let count = 0;
  for (const name of await readdir("/proc")) {
    if (!/^\d+$/u.test(name)) continue;
    let stat;
    let status;
    try {
      stat = await readFile(`/proc/${name}/stat`, "utf8");
      status = await readFile(`/proc/${name}/status`, "utf8");
    } catch (error) {
      if (error.code === "ENOENT" || error.code === "ESRCH") continue;
      throw error;
    }
    const fields = stat.slice(stat.lastIndexOf(")") + 2).trim().split(/\s+/u);
    if (Number(fields[2]) !== pgid) continue;
    count += 1;
    const memory = /^VmRSS:\s+(\d+) kB$/mu.exec(status);
    requireValue(memory || fields[0] === "Z", "live process RSS is unobservable");
    if (memory) rss += Number(memory[1]) * 1024;
  }
  requireValue(Number.isFinite(rss), "process-group RSS is unobservable");
  return { rss, count };
}

export function runningProblem({ rss, count, output, scratch }, bounds = BOUNDS) {
  if (![rss, count, output, scratch].every(Number.isFinite)) return "runtime usage is unobservable";
  if (rss > bounds.maxRss) return "process-group memory ceiling exceeded";
  if (count > bounds.maxProcesses) return "process-group PID ceiling exceeded";
  if (output > bounds.maxOutput) return "combined output ceiling exceeded";
  if (scratch > bounds.maxScratch) return "scratch byte ceiling exceeded";
  return null;
}
