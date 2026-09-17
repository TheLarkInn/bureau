import assert from "node:assert/strict";
import test from "node:test";

import { BOUNDS, GiB, admissionProblem, directoryBytes, runningProblem } from "./maintenance-resources.mjs";
import { childEnvironment } from "./maintenance-child.mjs";

function safeSnapshot() {
  return { platform: "linux", wsl: false, backingChecked: false,
    disks: [{ path: "/", free: 8 * GiB }, { path: "/work", free: 16 * GiB }],
    availableMemory: 16 * GiB,
    groups: [{ memoryMax: 8 * GiB, memoryCurrent: GiB, pidsMax: 128, pidsCurrent: 10, cpus: 2 }] };
}

test("admission requires observable bounded Linux resources on every relevant filesystem", () => {
  assert.equal(admissionProblem(safeSnapshot()), null);
  const snapshots = [
    { ...safeSnapshot(), platform: "win32" },
    { ...safeSnapshot(), wsl: true },
    { ...safeSnapshot(), disks: [] },
    { ...safeSnapshot(), disks: [{ path: "/", free: 176 * 1024 * 1024 }, { path: "/work", free: 400 * GiB }] },
    { ...safeSnapshot(), disks: [{ path: "/backing", free: NaN }] },
    { ...safeSnapshot(), availableMemory: 0 },
    { ...safeSnapshot(), groups: [] },
  ];
  for (const snapshot of snapshots) assert.equal(typeof admissionProblem(snapshot), "string");
});

test("finite limits and ancestor headroom are independently enforced", () => {
  for (const change of [
    { memoryMax: Infinity }, { memoryMax: 16 * GiB }, { pidsMax: Infinity },
    { cpus: Infinity }, { cpus: 8 }, { pidsCurrent: 120 }, { memoryCurrent: 7.5 * GiB },
    { memoryCurrent: NaN },
  ]) {
    const snapshot = safeSnapshot();
    Object.assign(snapshot.groups[0], change);
    assert.equal(typeof admissionProblem(snapshot), "string");
  }
  const snapshot = safeSnapshot();
  snapshot.groups.push({ memoryMax: 8 * GiB, memoryCurrent: 8 * GiB, pidsMax: 256, pidsCurrent: 5, cpus: 4 });
  assert.match(admissionProblem(snapshot), /memory headroom/u);
});

test("admission reserves the complete child RSS allowance plus headroom on host and cgroup", () => {
  const threshold = BOUNDS.maxRss + BOUNDS.memoryFloor;
  for (const field of ["host", "cgroup"]) {
    const snapshot = safeSnapshot();
    if (field === "host") snapshot.availableMemory = threshold;
    else snapshot.groups[0].memoryCurrent = snapshot.groups[0].memoryMax - threshold;
    assert.equal(admissionProblem(snapshot), null);
    if (field === "host") snapshot.availableMemory -= 1;
    else snapshot.groups[0].memoryCurrent += 1;
    assert.match(admissionProblem(snapshot), /memory headroom/u);
  }
});

test("disk threshold is exact; a WSL backing observation is not a native root exemption", () => {
  const snapshot = { ...safeSnapshot(), wsl: true, backingChecked: true };
  snapshot.disks[0].free = BOUNDS.diskFloor;
  assert.equal(admissionProblem(snapshot), null);
  snapshot.disks[0].free -= 1;
  assert.match(admissionProblem(snapshot), /disk floor.*\//u);
});

test("runtime memory, process, output and scratch ceilings fail explicitly", () => {
  const safe = { rss: 0, count: 0, output: 0, scratch: 0 };
  assert.equal(runningProblem(safe), null);
  for (const [key, limit] of [["rss", BOUNDS.maxRss], ["count", BOUNDS.maxProcesses],
    ["output", BOUNDS.maxOutput], ["scratch", BOUNDS.maxScratch]]) {
    assert.equal(runningProblem({ ...safe, [key]: limit }), null);
    assert.equal(typeof runningProblem({ ...safe, [key]: limit + 1 }), "string");
  }
  assert.match(runningProblem({ ...safe, rss: NaN }), /unobservable/u);
});

test("offline child environment forwards no ambient credential or runtime hooks", () => {
  const environment = childEnvironment({ BUREAU_CHAOS_SEED: "3", TMPDIR: "/scratch" });
  assert.deepEqual(Object.keys(environment).sort(), ["BUREAU_CHAOS_SEED", "HOME", "LANG", "PATH", "TMPDIR"]
    .filter((key) => key !== "HOME" || process.env.HOME).sort());
  assert.throws(() => childEnvironment({ GH_TOKEN: "not-authorized" }), /unapproved/u);
});

test("scratch accounting counts browser profile symlinks without following their targets", async () => {
  const visited = [];
  const bytes = await directoryBytes("/scratch", 1000, {
    async lstat(path) {
      visited.push(path);
      const directory = path === "/scratch";
      return { size: directory ? 100 : 30, isDirectory: () => directory, isSymbolicLink: () => !directory };
    },
    async readdir(path) {
      assert.equal(path, "/scratch");
      return ["SingletonLock"];
    },
  });
  assert.equal(bytes, 130);
  assert.equal(visited.length, 2);
});
