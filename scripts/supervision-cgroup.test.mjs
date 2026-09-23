import assert from "node:assert/strict";
import test from "node:test";

import { identity, managerRoot, serviceCgroup } from "../deployment/supervision/protocol.mjs";
import { checkedLease } from "../deployment/supervision/lease.mjs";
import { drain } from "../deployment/supervision/transaction.mjs";
import { drainProcessIdentity, emptyService, processIdentity } from "../deployment/supervision/system.mjs";
import { COMMIT, GUARD, OWNER } from "./supervision-test-support.mjs";

const WSL = "/wsl-user/distro-4668/systemd";
const ENGINE = { invocation: "4".repeat(32), pid: 789, starttime: "1011",
  cgroup: `${WSL}/system.slice/bureau-maintenance.service` };
const active = { ActiveState: "deactivating", MainPID: String(ENGINE.pid),
  InvocationID: ENGINE.invocation, ControlGroup: ENGINE.cgroup };
const terminal = { ...active, ActiveState: "inactive", MainPID: "0" };
const directory = { isDirectory: () => true, isSymbolicLink: () => false };
const missing = (code) => Object.assign(new Error(`synthetic observation: ${code}`), { code });

function statRecord(status = "S") {
  return `${ENGINE.pid} (bureau) ${[status, ...Array(18).fill("0"), ENGINE.starttime, "0", "0"].join(" ")}\n`;
}

function proc({ root = WSL, status = "S", group = `0::${ENGINE.cgroup}\n` } = {}) {
  return { root: async () => root, realpath: async () => "/opt/bureau/bin/bureau",
    readFile: async (path) => path.endsWith("/stat") ? statRecord(status) : group };
}

test("the manager root is PID 1's exact cgroup v2 path without init.scope", () => {
  const cases = [["0::/init.scope\n", ""], ["0::/wsl-user/distro-4668/systemd/init.scope\n", WSL],
    ["0::/a.b/c_d/e@f-g/init.scope\n", "/a.b/c_d/e@f-g"]];
  assert.deepEqual(cases.map(([text]) => managerRoot(text)), cases.map(([, root]) => root));
});

test("malformed, hybrid, traversing or non-init PID 1 cgroups refuse", () => {
  for (const text of ["", "\n", "0::/init.scope", "0::/\n", "0::/system.slice/init.scope.x\n",
    "0::init.scope\n", "0::/wsl-user//init.scope\n", "0::/wsl-user/../init.scope\n", "0::/./init.scope\n",
    "0:://init.scope\n", "0::/wsl user/init.scope\n", "1:name=systemd:/init.scope\n0::/init.scope\n",
    "0::/init.scope\n0::/init.scope\n", "0::/init.scope\n\n", "0::/a/init.scope (deleted)\n",
    "0::/init.scope/init.scope\x00\n", null, ["0::/init.scope\n"]]) {
    assert.throws(() => managerRoot(text), /manager cgroup root/u, JSON.stringify(text));
  }
});

test("service cgroups are exactly <manager root>/system.slice/<unit>.service", () => {
  const accepted = [["/system.slice/bureau-maintenance.service", ""], [ENGINE.cgroup, WSL]];
  const refused = [[ENGINE.cgroup, ""], ["/system.slice/bureau-maintenance.service", WSL],
    ["/wsl-user/distro-3850/systemd/system.slice/bureau-maintenance.service", WSL],
    [`${WSL}/user.slice/bureau-maintenance.service`, WSL], [`${WSL}/system.slice/nested/x.service`, WSL],
    [`${WSL}/system.slice/bureau.slice/x.service`, WSL], [`${WSL}/system.slice/../x.service`, WSL],
    [`${WSL}/system.slice/x.scope`, WSL], [`${WSL}/system.slice/.service`, WSL],
    [`/wsl-user/../systemd/system.slice/x.service`, "/wsl-user/../systemd"],
    ["//system.slice/x.service", "/"], ["x/system.slice/x.service", "x"],
    ["/system.slice/x.service", undefined], ["/system.slice/x.service", null], [["/system.slice/x.service"], ""]];
  assert.deepEqual([accepted.map((pair) => serviceCgroup(...pair)), refused.filter((pair) => serviceCgroup(...pair))],
    [[true, true], []]);
});

test("owned identity binds the observed manager root, not any prefix", () => {
  assert.deepEqual([identity(ENGINE, WSL), identity({ ...ENGINE, cgroup: "/system.slice/x.service" }, "")],
    [ENGINE, { ...ENGINE, cgroup: "/system.slice/x.service" }]);
  for (const root of ["", undefined, "/wsl-user/distro-18039/systemd", `${WSL}/system.slice`]) {
    assert.throws(() => identity(ENGINE, root), /identity/u);
  }
});

test("a guard lease under the WSL manager root requires that same root", () => {
  const lease = { guard: { ...GUARD, cgroup: `${WSL}/system.slice/bureau-windows-heartbeat.service` },
    owner: OWNER, commit: COMMIT, sequence: 0, at: 1000 };
  assert.equal(checkedLease(lease, 1100, COMMIT, WSL), lease);
  assert.throws(() => checkedLease(lease, 1100, COMMIT, ""), /identity/u);
});

test("live WSL identity keeps the exact kernel membership comparison", async () => {
  assert.deepEqual((await processIdentity(active, proc())).identity, ENGINE);
  for (const inspect of [proc({ root: "" }), proc({ group: "0::/system.slice/bureau-maintenance.service\n" }),
    proc({ group: `0::${WSL}/system.slice/other.service\n` })]) {
    await assert.rejects(processIdentity(active, inspect), /identity|escaped/u);
  }
});

test("WSL drain observes running and exited leaders and refuses a foreign root", async () => {
  const observed = await Promise.all([drainProcessIdentity(active, ENGINE, proc()),
    drainProcessIdentity(active, ENGINE, proc({ status: "Z", group: "0::/\n" }))]);
  assert.deepEqual(observed.map((value) => value.status), ["running", "exited"]);
  for (const inspect of [proc({ root: "" }), { ...proc(), root: async () => { throw missing("ENOENT"); } }]) {
    await assert.rejects(drainProcessIdentity(active, ENGINE, inspect), /identity|synthetic/u);
  }
});

test("WSL emptiness proof reads the full /sys/fs/cgroup path for the owned group", async () => {
  const paths = [];
  const inspect = { show: async () => terminal, root: async () => WSL, lstat: async () => directory,
    readFile: async (path) => { paths.push(path); return "populated 0\n"; } };
  assert.deepEqual(await emptyService("bureau-maintenance.service", ENGINE, inspect), terminal);
  assert.deepEqual(paths, [`/sys/fs/cgroup${ENGINE.cgroup}/cgroup.events`]);
});

test("collected WSL cgroup proof checks the prefixed system.slice parent", async () => {
  const seen = [];
  const inspect = { show: async () => terminal, root: async () => WSL, readFile: async () => { throw missing("ENOENT"); },
    lstat: async (path) => {
      seen.push(path);
      if (path === `/sys/fs/cgroup${ENGINE.cgroup}`) throw missing("ENOENT");
      return directory;
    } };
  await emptyService("bureau-maintenance.service", ENGINE, inspect);
  assert.deepEqual(seen, [`/sys/fs/cgroup${ENGINE.cgroup}`, `/sys/fs/cgroup${WSL}/system.slice`]);
});

test("emptiness proof refuses a group outside the observed manager root", async () => {
  for (const [root, cgroup] of [["", ENGINE.cgroup], [WSL, "/system.slice/bureau-maintenance.service"],
    ["/wsl-user/distro-3850/systemd", ENGINE.cgroup], [WSL, `${WSL}/user.slice/x.service`]]) {
    await assert.rejects(emptyService("bureau-maintenance.service", { ...ENGINE, cgroup }, {
      show: async () => ({ ...terminal, ControlGroup: "" }), root: async () => root,
      readFile: async () => "populated 0\n", lstat: async () => directory,
    }), /unobservable/u);
  }
});

test("actual drain completes through the real WSL process and cgroup verifiers", async () => {
  let reads = 0;
  const samples = [proc(), proc({ status: "Z", group: "0::/\n" })];
  const inspect = { show: async () => terminal, root: async () => WSL, lstat: async () => directory,
    readFile: async () => "populated 0\n" };
  await drain("bureau-maintenance.service", ENGINE, {
    show: async () => ++reads <= samples.length ? active : terminal,
    inspect: (state, owned) => drainProcessIdentity(state, owned, samples[reads - 1]),
    empty: (unit, owned) => emptyService(unit, owned, inspect), now: () => reads, wait: async () => {}, timeout: 10,
  });
  assert.equal(reads, 3);
});
