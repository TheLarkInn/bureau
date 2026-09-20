import assert from "node:assert/strict";
import test from "node:test";

import { drain } from "../deployment/supervision/transaction.mjs";
import { drainProcessIdentity, emptyService, processIdentity } from "../deployment/supervision/system.mjs";
import { ENGINE } from "./supervision-test-support.mjs";

const active = { ActiveState: "deactivating", MainPID: String(ENGINE.pid),
  InvocationID: ENGINE.invocation, ControlGroup: ENGINE.cgroup };
const terminal = { ...active, ActiveState: "inactive", MainPID: "0" };
const missing = (code) => Object.assign(new Error(`synthetic process observation: ${code}`), { code });

function statRecord({ status = "S", pid = ENGINE.pid, starttime = ENGINE.starttime } = {}) {
  return `${pid} (synthetic ) leader) ${[status, ...Array(18).fill("0"), starttime, "0", "0"].join(" ")}\n`;
}

function proc({ stat = statRecord(), group = `0::${ENGINE.cgroup}\n`, stage, code = "ENOENT" } = {}) {
  const calls = [];
  const observe = (path, value) => {
    const name = path.split("/").at(-1);
    calls.push(name);
    if (name === stage) throw missing(code);
    return value;
  };
  return { calls, readFile: async (path) => observe(path, path.endsWith("/stat") ? stat : group),
    realpath: async (path) => observe(path, "/opt/bureau/bin/bureau") };
}

test("drain observes matching live and exited leaders without requiring a zombie executable", async () => {
  for (const status of ["Z", "X", "x"]) {
    const inspect = proc({ stat: statRecord({ status }), group: "0::/\n", stage: "exe" });
    const observed = await drainProcessIdentity(active, ENGINE, inspect);
    assert.deepEqual(observed, { status: "exited", identity: ENGINE });
    assert.deepEqual(inspect.calls, ["stat"]);
  }
  assert.deepEqual(await drainProcessIdentity(active, ENGINE, proc()), { status: "running", identity: ENGINE });
});

test("vanished stat, cgroup or executable is drain progress, never a live identity", async () => {
  for (const stage of ["stat", "cgroup", "exe"]) {
    for (const code of ["ENOENT", "ESRCH"]) {
      const inspect = proc({ stage, code });
      assert.deepEqual(await drainProcessIdentity(active, ENGINE, inspect), { status: "absent", identity: null });
      await assert.rejects(processIdentity(active, inspect), { code });
    }
  }
  for (const status of ["Z", "X", "x"]) {
    await assert.rejects(processIdentity(active, proc({ stat: statRecord({ status }) })), /absent or dead/u);
  }
});

test("actual drain retries live to zombie to disappeared transitions before terminal cgroup proof", async () => {
  for (const status of ["Z", "X"]) {
    let reads = 0;
    let verified = 0;
    const samples = [proc(), proc({ stat: statRecord({ status }), stage: "exe" }),
      proc({ stage: "stat" }), proc({ stage: "exe", code: "ESRCH" })];
    await drain("engine.service", ENGINE, {
      show: async () => ++reads <= samples.length ? active : terminal,
      inspect: (state, owned) => drainProcessIdentity(state, owned, samples[reads - 1]),
      empty: async () => { assert.equal(reads, 5); verified += 1; return terminal; },
      now: () => reads, wait: async () => {}, timeout: 10,
    });
    assert.deepEqual([reads, verified], [5, 1]);
  }
});

test("absent and exited leaders cannot shorten the drain deadline or prove cgroup emptiness", async () => {
  for (const inspect of [proc({ stage: "stat" }), proc({ stat: statRecord({ status: "Z" }) })]) {
    let now = 0;
    let checked = false;
    await assert.rejects(drain("engine.service", ENGINE, {
      show: async () => active, inspect: (state, owned) => drainProcessIdentity(state, owned, inspect),
      now: () => now++, wait: async () => {}, timeout: 3,
      empty: async () => { checked = true; return terminal; },
    }), /not confirmed/u);
    assert.equal(checked, false);
  }
  for (const error of [missing("EACCES"), new Error("owned cgroup is still populated")]) {
    await assert.rejects(drain("engine.service", ENGINE, { show: async () => terminal,
      empty: async () => { throw error; } }), error);
  }
});

test("drain rejects foreign systemd ownership before accepting a missing process", async () => {
  for (const patch of [{ InvocationID: "f".repeat(32) }, { MainPID: String(ENGINE.pid + 1) },
    { ControlGroup: "/system.slice/foreign.service" }, { InvocationID: "" }, { MainPID: "0" }]) {
    const inspect = proc({ stage: "stat" });
    await assert.rejects(drainProcessIdentity({ ...active, ...patch }, ENGINE, inspect), /changed|no owned/u);
    assert.deepEqual(inspect.calls, []);
  }
});

test("drain validates an exited starttime and all observable identity before tolerating disappearance", async () => {
  for (const status of ["S", "Z", "X"]) {
    const inspect = proc({ stat: statRecord({ status, starttime: `${ENGINE.starttime}1` }), stage: "exe" });
    await assert.rejects(drainProcessIdentity(active, ENGINE, inspect), /process changed/u);
    assert.deepEqual(inspect.calls, ["stat"]);
  }
  await assert.rejects(drainProcessIdentity(active, ENGINE, proc({
    group: "0::/system.slice/foreign.service\n", stage: "exe",
  })), /escaped/u);
});

test("a leader exiting between stat and cgroup reads requires a matching fresh exited record", async () => {
  for (const status of ["Z", "X"]) {
    let reads = 0;
    const inspect = { ...proc(), readFile: async (path) => path.endsWith("/stat")
      ? statRecord({ status: reads++ === 0 ? "S" : status }) : "0::/\n" };
    assert.deepEqual(await drainProcessIdentity(active, ENGINE, inspect), { status: "exited", identity: ENGINE });
    assert.equal(reads, 2);
  }
  for (const change of [{ status: "S" }, { status: "Z", starttime: `${ENGINE.starttime}1` }, { status: "?" }]) {
    let reads = 0;
    await assert.rejects(drainProcessIdentity(active, ENGINE, { ...proc(),
      readFile: async (path) => path.endsWith("/stat") ? statRecord(reads++ === 0 ? {} : change) : "0::/\n",
    }), /escaped|changed|malformed/u);
  }
  await assert.rejects(processIdentity(active, proc({ group: "0::/\n" })), /escaped/u);
});

test("permissions, IO failure and corrupt identity remain fatal during drain", async () => {
  for (const stage of ["stat", "cgroup", "exe"]) {
    for (const code of ["EACCES", "EPERM", "EIO"]) {
      await assert.rejects(drainProcessIdentity(active, ENGINE, proc({ stage, code })), { code });
    }
  }
  for (const stat of ["missing fields", statRecord({ status: "?" }), statRecord({ pid: ENGINE.pid + 1 }),
    statRecord({ starttime: "0" }), statRecord({ starttime: "corrupt" })]) {
    await assert.rejects(drainProcessIdentity(active, ENGINE, proc({ stat })), /malformed|unobservable/u);
  }
});

test("a changed invocation or cgroup in the final fresh observation cannot produce a drain proof", async () => {
  for (const patch of [{ InvocationID: "f".repeat(32) }, { ControlGroup: "/system.slice/foreign.service" }]) {
    await assert.rejects(drain("engine.service", ENGINE, {
      show: async () => terminal, empty: async () => ({ ...terminal, ...patch }),
    }), /changed during final drain observation/u);
  }
});

test("real emptiness verifier still checks the owned cgroup when systemd clears its metadata", async () => {
  const cleared = { ...terminal, ControlGroup: "", InvocationID: "" };
  for (const populated of ["populated 1\n", "", "populated 0\npopulated 1\n", "populated unknown\n"]) {
    await assert.rejects(emptyService("engine.service", ENGINE, {
      show: async () => cleared, readFile: async () => populated,
    }), /populated|unobservable/u);
  }
  let reads = 0;
  const result = await emptyService("engine.service", ENGINE, {
    show: async () => { reads += 1; return cleared; },
    readFile: async (path) => {
      assert.equal(path, `/sys/fs/cgroup${ENGINE.cgroup}/cgroup.events`);
      return "populated 0\nfrozen 0\n";
    },
  });
  assert.deepEqual([reads, result], [2, cleared]);
});

test("collected cgroup proof requires actual directory absence and an observable parent", async () => {
  const directory = { isDirectory: () => true, isSymbolicLink: () => false };
  const inspect = { show: async () => terminal, readFile: async () => { throw missing("ENOENT"); },
    lstat: async (path) => {
      if (path === `/sys/fs/cgroup${ENGINE.cgroup}`) throw missing("ENOENT");
      assert.equal(path, "/sys/fs/cgroup/system.slice");
      return directory;
    } };
  assert.deepEqual(await emptyService("engine.service", ENGINE, inspect), terminal);
  await assert.rejects(emptyService("engine.service", ENGINE, { ...inspect, lstat: async () => directory }), /counter/u);
  for (const code of ["ENOENT", "EACCES", "EIO"]) {
    await assert.rejects(emptyService("engine.service", ENGINE, {
      ...inspect, lstat: async () => { throw missing(code); },
    }), { code });
  }
  await assert.rejects(emptyService("engine.service", ENGINE, {
    ...inspect, readFile: async () => { throw missing("EACCES"); },
  }), { code: "EACCES" });
});

test("real emptiness proof rechecks terminal ownership after observing the cgroup", async () => {
  for (const patch of [{ ActiveState: "active" }, { MainPID: String(ENGINE.pid) },
    { InvocationID: "f".repeat(32) }, { ControlGroup: "/system.slice/foreign.service" }]) {
    let reads = 0;
    await assert.rejects(emptyService("engine.service", ENGINE, {
      show: async () => reads++ === 0 ? terminal : { ...terminal, ...patch },
      readFile: async () => "populated 0\n",
    }), /not drained|ownership changed/u);
    assert.equal(reads, 2);
  }
});
