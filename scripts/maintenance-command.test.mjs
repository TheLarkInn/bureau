import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { ACQUIRED, ACQUIRED_FD, admittedCommand, lockedCommand, waitingBounds } from "./maintenance-command.mjs";
import { BOUNDS, GiB, MiB } from "./maintenance-resources.mjs";

const JOB = { args: ["--user", "--pid", "command with spaces", 'argument with "\''],
  context: { cwd: "/source", backingPaths: ["/backing"], extraPaths: ["/cache"] },
  bounds: { ...BOUNDS, maxRss: 4 * GiB } };
const hasFlock = process.platform === "linux" && (() => {
  try {
    execFileSync("flock", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
})();

test("shared command contention waits only for the caller's bounded lock wait", () => {
  for (const [waitMs, seconds] of [[0, "0"], [1000, "1"], [480_000, "480"]]) {
    const args = lockedCommand("/cache/command.lock", JOB.args, JOB.context, JOB.bounds, waitMs);
    assert.deepEqual([args.slice(0, 5), args.includes("--nonblock") || args.includes("--no-fork"),
      JSON.parse(args.at(-1))], [["--wait", seconds, "--conflict-exit-code", "75", "/cache/command.lock"],
      false, JOB]);
  }
  for (const waitMs of [-1, 1.5, undefined]) {
    assert.throws(() => lockedCommand("/cache/command.lock", JOB.args, JOB.context, JOB.bounds, waitMs),
      /invalid command lock wait/u);
  }
});

test("waiting changes only the memory reservation, never disk, process or output limits", () => {
  assert.deepEqual(waitingBounds(JOB.bounds), { ...JOB.bounds, maxRss: 0, memoryFloor: 256 * MiB });
});

test("acquisition is reported, then full headroom is rechecked, before replacing the command", async () => {
  const calls = [];
  const replaced = new Error("test observed process replacement");
  await assert.rejects(admittedCommand(JOB, {
    acquired: () => calls.push(["acquired"]),
    async check(context, bounds) {
      calls.push(["admit", context, bounds]);
      await Promise.resolve();
    },
    replace(file, args, environment) {
      calls.push(["exec", file, args, environment]);
      throw replaced;
    },
    environment: { PATH: "/usr/bin:/bin" },
  }), (error) => error === replaced);
  assert.deepEqual(calls, [["acquired"], ["admit", JOB.context, JOB.bounds],
    ["exec", "/bin/sh", ["sh", "-c", 'exec unshare "$@"', "maintenance", ...JOB.args],
      { PATH: "/usr/bin:/bin" }]]);
});

test("capacity lost while waiting prevents execution rather than reusing the earlier admission", async () => {
  let executions = 0;
  await assert.rejects(admittedCommand(JOB, {
    acquired: () => {},
    check: async () => { throw new Error("cgroup memory headroom below required reserve"); },
    replace: () => { executions += 1; },
  }), /memory headroom/u);
  assert.equal(executions, 0);
});

test("missing process replacement or a returning replacement never reports success", async () => {
  let reported = 0;
  await assert.rejects(admittedCommand(JOB, { acquired: () => { reported += 1; }, replace: null }),
    /process.execve is required/u);
  await assert.rejects(admittedCommand(JOB, {
    acquired: () => {}, check: async () => {}, replace: () => {},
  }), /without replacing/u);
  assert.equal(reported, 0);
});

test("execution failures propagate once and never trigger a retry of an executed check", async () => {
  let attempts = 0;
  const failure = Object.assign(new Error("exec failed"), { code: "ENOENT" });
  await assert.rejects(admittedCommand(JOB, {
    acquired: () => {}, check: async () => {},
    replace: () => { attempts += 1; throw failure; },
  }), (error) => error === failure);
  assert.equal(attempts, 1);
});

async function lockedRun(lock, cwd, waitMs) {
  const stdio = ["ignore", "ignore", "ignore"];
  stdio[ACQUIRED_FD] = "pipe";
  // `unshare --version` is inert if the host admits it after acquisition.
  const child = spawn("flock", lockedCommand(lock, ["--version"],
    { cwd, backingPaths: [], extraPaths: [] }, BOUNDS, waitMs), { stdio });
  let signal = "";
  child.stdio[ACQUIRED_FD].on("data", (chunk) => { signal += chunk; });
  const [code] = await once(child, "close");
  return { code, signal };
}

test("flock reports acquisition on the dedicated descriptor only once it holds the lock",
  { skip: !hasFlock }, async () => {
    const base = await mkdtemp(join(tmpdir(), "bureau-command-lock-"));
    const lock = join(base, "command.lock");
    const holder = spawn("flock", [lock, "-c", "echo held; exec sleep 30"],
      { detached: true, stdio: ["ignore", "pipe", "ignore"] });
    await once(holder.stdout, "data");
    const busy = await lockedRun(lock, base, 0);
    process.kill(-holder.pid, "SIGKILL");
    holder.stdout.destroy();
    const free = await lockedRun(lock, base, 5000);
    await rm(base, { recursive: true });
    assert.deepEqual([busy, free.signal], [{ code: 75, signal: "" }, ACQUIRED]);
  });
