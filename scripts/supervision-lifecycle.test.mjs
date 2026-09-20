import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { readBoundedJson } from "./maintenance-files.mjs";
import { claim, checkedLease, publish } from "../deployment/supervision/lease.mjs";
import { drain, unitArguments } from "../deployment/supervision/transaction.mjs";
import { approval, ENV, processIdentity } from "../deployment/supervision/system.mjs";
import { binaryApproval, binaryDigest } from "../deployment/supervision/provenance.mjs";
import { COMMIT, ENGINE, GUARD, OWNER } from "./supervision-test-support.mjs";

const lease = () => ({ guard: GUARD, owner: OWNER, commit: COMMIT, sequence: 0, at: 1000 });

test("native provenance requires explicit approval, matching source and actual bounded binary bytes", async (t) => {
  const runtime = await mkdtemp(join(tmpdir(), "bureau-provenance-"));
  t.after(() => rm(runtime, { recursive: true, force: true }));
  const path = join(runtime, "synthetic-executable");
  const bytes = "synthetic fixture, not a Bureau executable";
  await writeFile(path, bytes);
  const expected = createHash("sha256").update(bytes).digest("hex");
  const value = { schema: "bureau-native-supervision-v1", approved: true, commit: COMMIT,
    bureau_sha256: expected, node_path: "/opt/bureau/bin/node", node_sha256: expected };
  assert.equal(await binaryDigest(path), binaryApproval(value, COMMIT));
  for (const patch of [{ approved: false }, { commit: "0".repeat(40) }, { bureau_sha256: "" }]) {
    assert.throws(() => binaryApproval({ ...value, ...patch }, COMMIT), /provenance/u);
  }
  await writeFile(path, "");
  await assert.rejects(binaryDigest(path), /byte bound/u);
});

test("actual one-use admission file refuses concurrent and replacement invocations", async (t) => {
  const runtime = await mkdtemp(join(tmpdir(), "bureau-supervision-"));
  t.after(() => rm(runtime, { recursive: true, force: true }));
  await publish(runtime, "lease.json", lease());
  const context = { runtime, engine: "engine.service", guard: "guard.service", commit: COMMIT,
    invocation: ENGINE.invocation, pid: 321, executable: "/opt/bureau/bin/node" };
  const effects = { now: () => 1100, inspect: async (state) => state.ActiveState === "active" ? { identity: GUARD }
    : { identity: { ...ENGINE, invocation: context.invocation, pid: context.pid }, executable: context.executable },
    show: async (unit) => unit === context.guard ? { ActiveState: "active", SubState: "running" }
      : { ActiveState: "activating", SubState: "start-pre", InvocationID: context.invocation, ControlPID: String(context.pid) } };
  const results = await Promise.allSettled([claim(context, effects), claim(context, effects)]);
  assert.deepEqual(results.map((value) => value.status).sort(), ["fulfilled", "rejected"]);
  context.invocation = "6".repeat(32);
  await assert.rejects(claim(context, effects), { code: "EEXIST" });
  assert.equal((await readBoundedJson(join(runtime, "claim.json"))).invocation, ENGINE.invocation);
});

test("startup grant rejects stale ownership, mismatched guardian and unrelated start", async () => {
  const context = { runtime: "/not-used", engine: "engine.service", guard: "guard.service",
    commit: COMMIT, invocation: ENGINE.invocation, pid: 321, executable: "/opt/bureau/bin/node" };
  const base = { now: () => 1100, inspect: async (state) => state.ActiveState === "active" ? { identity: GUARD }
    : { identity: { ...ENGINE, pid: 321 }, executable: context.executable }, read: async () => lease(),
    save: async () => { throw new Error("unexpected admission"); },
    show: async (unit) => unit === context.guard ? { ActiveState: "active", SubState: "running" }
      : { ActiveState: "activating", SubState: "start-pre", InvocationID: ENGINE.invocation, ControlPID: "321" } };
  for (const change of [{ now: () => 8000 }, { inspect: async () => ({ identity: ENGINE }) },
    { show: async () => ({ ActiveState: "inactive" }) }, { read: async () => ({ ...lease(), commit: "0".repeat(40) }) }]) {
    await assert.rejects(claim(context, { ...base, ...change }), /stale|identity|startup/u);
  }
  await assert.rejects(claim({ ...context, pid: 322 }, base), /owned service/u);
  await assert.rejects(claim(context, { ...base, live: () => false }), /live systemd/u);
  await assert.rejects(claim(context, { ...base, inspect: async (state) =>
    state.ActiveState === "active" ? { identity: GUARD }
      : { identity: { ...ENGINE, pid: 321 }, executable: "/usr/bin/unqualified-node" } }), /qualified interpreter/u);
});

test("lease age is monotonic, finite and bounded; future and missing counters are refusal", () => {
  for (const value of [{ ...lease(), at: 1101 }, { ...lease(), at: NaN }, { ...lease(), at: undefined },
    { ...lease(), sequence: -1 }, { ...lease(), owner: "" }]) {
    assert.throws(() => checkedLease(value, 1100, COMMIT), /stale/u);
  }
});

test("lease textual fields reject JSON type coercion before startup publication", () => {
  for (const field of ["owner", "commit"]) {
    for (const value of [[lease()[field]], {}, null, true, 123]) {
      assert.throws(() => checkedLease({ ...lease(), [field]: value }, 1100, value), /stale/u);
    }
  }
});

test("drain waits for exact owned service and cgroup emptiness without any stop-by-name effect", async () => {
  let reads = 0;
  let verified = false;
  const states = [{ ActiveState: "deactivating", MainPID: `${ENGINE.pid}`, InvocationID: ENGINE.invocation },
    { ActiveState: "inactive", MainPID: "0", InvocationID: "" }];
  await drain("engine.service", ENGINE, { show: async () => states[Math.min(reads++, 1)],
    inspect: async () => ({ identity: ENGINE }), now: () => reads, wait: async () => {},
    empty: async () => { verified = true; return states[1]; }, timeout: 10 });
  assert.deepEqual([reads, verified], [2, true]);
});

test("identity drift during shutdown never targets a replacement by name", async () => {
  await assert.rejects(drain("engine.service", ENGINE, {
    show: async () => ({ ActiveState: "active", MainPID: "1234", InvocationID: OWNER }),
    empty: async () => { throw new Error("must not accept replacement"); },
  }), /changed during drain/u);
  for (const patch of [{ pid: 900 }, { starttime: "1012" }, { cgroup: "/system.slice/other.service" }]) {
    await assert.rejects(drain("engine.service", ENGINE, {
      show: async () => ({ ActiveState: "deactivating", MainPID: `${ENGINE.pid}`, InvocationID: ENGINE.invocation }),
      inspect: async () => ({ identity: { ...ENGINE, ...patch } }),
    }), /process changed during drain/u);
  }
});

test("unknown and populated cgroups cannot be called drained; deadlines are absolute", async () => {
  let now = 0;
  await assert.rejects(drain("engine.service", ENGINE, {
    show: async () => ({ ActiveState: "deactivating", MainPID: `${ENGINE.pid}`, InvocationID: ENGINE.invocation }),
    inspect: async () => ({ identity: ENGINE }), now: () => now++, wait: async () => {}, timeout: 2,
  }), /not confirmed/u);
  await assert.rejects(drain("engine.service", ENGINE, {
    show: async () => ({ ActiveState: "inactive", MainPID: "0", InvocationID: "" }),
    empty: async () => { throw new Error("still populated"); },
  }), /populated/u);
});

test("real process identity parser retains starttime and rejects missing PID or cgroup drift", async () => {
  const state = { MainPID: `${ENGINE.pid}`, InvocationID: ENGINE.invocation, ControlGroup: ENGINE.cgroup };
  const fields = ["S", ...Array(18).fill("0"), ENGINE.starttime, "0", "0"];
  const inspect = { realpath: async () => "/opt/bureau/bin/bureau",
    readFile: async (path) => path.endsWith("/stat") ? `${ENGINE.pid} (a ) name) ${fields.join(" ")}`
      : `0::${ENGINE.cgroup}\n` };
  assert.deepEqual((await processIdentity(state, inspect)).identity, ENGINE);
  await assert.rejects(processIdentity({ ...state, MainPID: "0" }, inspect), /no owned identity/u);
  await assert.rejects(processIdentity({ ...state, ControlGroup: "/system.slice/unrelated.service" }, inspect), /escaped/u);
});

test("approval uses explicit reviewed commit only, with no environment escape hatch", () => {
  approval(`BUREAU_DEPLOYMENT_APPROVED=true\nBUREAU_CONFIG_COMMIT=${COMMIT}`, COMMIT);
  for (const value of [`BUREAU_DEPLOYMENT_APPROVED=false\nBUREAU_CONFIG_COMMIT=${COMMIT}`,
    `BUREAU_DEPLOYMENT_APPROVED=true\nBUREAU_CONFIG_COMMIT=${COMMIT}\nNODE_OPTIONS=override`]) {
    assert.throws(() => approval(value, COMMIT), /approval/u);
  }
  assert.deepEqual(Object.keys(ENV).sort(),
    ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM", "GIT_OPTIONAL_LOCKS", "HOME", "LANG", "LC_ALL", "PATH"]);
});

test("opt-in drop-in preserves the sole engine command and shortens only supervised shutdown", async () => {
  const ordinary = await readFile(new URL("../deployment/bureau-maintenance.service", import.meta.url), "utf8");
  const profile = await readFile(new URL("../deployment/windows-supervision.conf", import.meta.url), "utf8");
  assert.match(ordinary, /TimeoutStopSec=70min\nRestart=on-failure/u);
  assert.match(profile, /RefuseManualStart=yes[\s\S]*Restart=no[\s\S]*TimeoutStopSec=5s/u);
  assert.equal(profile.includes("ExecStart="), false);
  const args = unitArguments({ args: [COMMIT] });
  assert.equal(args.some((value) => value.includes("run-owner.sh") || value.includes("reconcile")), false);
  assert.equal(args.includes("--property=WatchdogSec=12s"), true);
});
