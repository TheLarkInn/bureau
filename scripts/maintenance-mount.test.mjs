import assert from "node:assert/strict";
import test from "node:test";

import { descriptorMountId, filesystemSnapshot, readOnlyMount } from "./maintenance-mount.mjs";
import { backingVerified } from "./maintenance-resources.mjs";

function mount(id, path, mode) {
  return `${id} 1 8:1 / ${path} ${mode},relatime - ext4 /dev/disk rw`;
}

function fixture({ table = mount(21, "/tools", "ro"), id = "21", metadata } = {}) {
  const calls = [];
  const io = {
    async openPath(path, flags) {
      calls.push(["open", path, flags]);
      return { fd: 7, stat: async () => metadata ?? {
        dev: 9n, isDirectory: () => true, isSymbolicLink: () => false,
      }, close: async () => { calls.push(["close"]); } };
    },
    async read(path) {
      calls.push(["read", path]);
      return path.includes("fdinfo") ? `pos:\t0\nmnt_id:\t${id}\n` : table;
    },
    async capacity(path) {
      calls.push(["capacity", path]);
      return { bavail: 10n, bsize: 4096n, type: 0xef53n };
    },
  };
  return { calls, io };
}

test("effective descriptor identity, never row order, distinguishes RO/RW overmounts", () => {
  for (const [hidden, active, expected] of [["ro", "rw", false], ["rw", "ro", true]]) {
    const rows = [mount(20, "/tools", hidden), mount(21, "/tools", active)];
    for (const table of [rows, rows.toReversed()]) {
      assert.equal(readOnlyMount(table.join("\n"), "/tools/pkg", 21), expected);
    }
  }
});

test("mount path escapes and descriptor IDs are strict and source-bound", () => {
  const table = mount(21, "/tools\\040with\\134slash", "ro");
  assert.equal(readOnlyMount(table, "/tools with\\slash/pkg", 21), true);
  for (const info of ["", "mnt_id: nope\n", "mnt_id: 0\n", "mnt_id: 21\nmnt_id: nope\n"]) {
    assert.throws(() => descriptorMountId(info), /identity/u);
  }
  for (const id of [undefined, 999]) assert.throws(() => readOnlyMount(table, "/tools", id));
  assert.throws(() => readOnlyMount(table, "/another/path", 21), /does not contain/u);
  assert.throws(() => readOnlyMount(mount(21, "/bad\\777path", "ro"), "/bad", 21), /escape/u);
  assert.throws(() => readOnlyMount(`${table}\n${table}`, "/tools with\\slash", 21), /duplicate/u);
});

test("capacity and effective mount proof use the same live descriptor and always close it", async () => {
  const { io, calls } = fixture();
  const result = await filesystemSnapshot("/tools", io);
  assert.deepEqual(result, { path: "/tools", free: 40960, type: 0xef53,
    device: "9", mountId: 21, readOnly: true });
  assert.equal(calls.some(([action, path]) => action === "capacity" && path === "/proc/self/fd/7"), true);
  assert.deepEqual(calls.at(-1), ["close"]);
  assert.deepEqual(calls[0], ["open", "/tools", 0o10000000 | 0o400000]);
});

test("missing and inaccessible paths never fall back to a read-only ancestor", async () => {
  for (const code of ["ENOENT", "EACCES", "EPERM"]) {
    const error = Object.assign(new Error(code), { code });
    let opens = 0;
    await assert.rejects(filesystemSnapshot("/tools/missing", {
      openPath: async () => { opens += 1; throw error; },
    }), (observed) => observed === error);
    assert.equal(opens, 1);
  }
});

test("malformed proofs, symlinks and descriptor read failures close and fail", async () => {
  for (const options of [{ id: "bad" }, { id: "999" }, { table: "malformed" },
    { metadata: { dev: 1n, isDirectory: () => false, isSymbolicLink: () => true } }]) {
    const { io, calls } = fixture(options);
    await assert.rejects(filesystemSnapshot("/tools", io));
    assert.deepEqual(calls.at(-1), ["close"]);
  }
  const { io, calls } = fixture();
  io.read = async () => { throw Object.assign(new Error("unreadable proof"), { code: "EACCES" }); };
  await assert.rejects(filesystemSnapshot("/tools", io), /unreadable proof/u);
  assert.deepEqual(calls.at(-1), ["close"]);
});

test("a failed proof keeps its descriptor alive until other metadata reads settle", async () => {
  const { io } = fixture();
  const original = io.openPath;
  let capacityFinished = false;
  io.openPath = async (...args) => {
    const handle = await original(...args);
    handle.close = async () => { assert.equal(capacityFinished, true); };
    return handle;
  };
  io.capacity = async () => {
    await new Promise((resolve) => setTimeout(resolve, 5));
    capacityFinished = true;
    return { bavail: 1n, bsize: 4096n, type: 0xef53n };
  };
  io.read = async () => { throw new Error("proof read failed"); };
  await assert.rejects(filesystemSnapshot("/tools", io), /proof read failed/u);
});

test("backing admission requires that same descriptor's read-only and distinct-device proof", async () => {
  const root = { path: "/", device: "1", readOnly: true };
  for (const [active, device, expected] of [["rw", "9", false], ["ro", "1", false], ["ro", "9", true]]) {
    const { io } = fixture({ table: [mount(20, "/tools", "ro"), mount(21, "/tools", active)].join("\n") });
    const disk = { ...await filesystemSnapshot("/tools", io), device };
    assert.equal(backingVerified(["/tools"], [root, disk]), expected);
  }
  assert.throws(() => backingVerified(["/missing"], [root]), /proof/u);
  assert.throws(() => backingVerified(["/tools"], [{ ...root, device: "unknown" }]), /identity/u);
});
