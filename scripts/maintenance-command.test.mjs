import assert from "node:assert/strict";
import test from "node:test";

import { admittedCommand, lockedCommand, waitingBounds } from "./maintenance-command.mjs";
import { BOUNDS, GiB, MiB } from "./maintenance-resources.mjs";

const JOB = { args: ["--user", "--pid", "command with spaces", 'argument with "\''],
  context: { cwd: "/source", backingPaths: ["/backing"], extraPaths: ["/cache"] },
  bounds: { ...BOUNDS, maxRss: 4 * GiB } };

test("shared command contention waits within the existing deadline without nonblocking retries", () => {
  for (const timeout of [1, 150_000, 900_000]) {
    const args = lockedCommand("/cache/command.lock", JOB.args, JOB.context, JOB.bounds, timeout);
    assert.deepEqual(args.slice(0, 5), ["--wait", String(timeout / 1000),
      "--conflict-exit-code", "75", "/cache/command.lock"]);
    assert.equal(args.includes("--nonblock") || args.includes("--no-fork"), false);
    assert.deepEqual(JSON.parse(args.at(-1)), JOB);
  }
});

test("waiting changes only the memory reservation, never disk, process or output limits", () => {
  assert.deepEqual(waitingBounds(JOB.bounds), { ...JOB.bounds, maxRss: 0, memoryFloor: 256 * MiB });
});

test("full current headroom is rechecked after acquisition and before replacing the command", async () => {
  const calls = [];
  const replaced = new Error("test observed process replacement");
  await assert.rejects(admittedCommand(JOB, {
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
  assert.deepEqual(calls, [["admit", JOB.context, JOB.bounds],
    ["exec", "/bin/sh", ["sh", "-c", 'exec unshare "$@"', "maintenance", ...JOB.args],
      { PATH: "/usr/bin:/bin" }]]);
});

test("capacity lost while waiting prevents execution rather than reusing the earlier admission", async () => {
  let executions = 0;
  await assert.rejects(admittedCommand(JOB, {
    check: async () => { throw new Error("cgroup memory headroom below required reserve"); },
    replace: () => { executions += 1; },
  }), /memory headroom/u);
  assert.equal(executions, 0);
});

test("missing process replacement or a returning replacement never reports success", async () => {
  await assert.rejects(admittedCommand(JOB, { replace: null }), /process.execve is required/u);
  await assert.rejects(admittedCommand(JOB, {
    check: async () => {}, replace: () => {},
  }), /without replacing/u);
});

test("execution failures propagate once and never trigger a retry of an executed check", async () => {
  let attempts = 0;
  const failure = Object.assign(new Error("exec failed"), { code: "ENOENT" });
  await assert.rejects(admittedCommand(JOB, {
    check: async () => {},
    replace: () => { attempts += 1; throw failure; },
  }), (error) => error === failure);
  assert.equal(attempts, 1);
});
