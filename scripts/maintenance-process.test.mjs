import assert from "node:assert/strict";
import test from "node:test";

import { parsePageSize, parseProcessStat, processGroupUsage } from "./maintenance-process.mjs";
import { BOUNDS, MiB, runningProblem } from "./maintenance-resources.mjs";

function stat({ pid = 123, parent = 0, group = 42, started = 1, state = "R",
  pages = "2", name = "test (process)" } = {}) {
  const fields = Array(22).fill("0");
  fields[0] = state;
  fields[1] = String(parent);
  fields[2] = String(group);
  fields[19] = String(started);
  fields[21] = pages;
  return `${pid} (${name}) ${fields.join(" ")}\n`;
}

test("RSS uses the actual observed page size, not an assumed 4 KiB", () => {
  assert.deepEqual(parseProcessStat(stat(), parsePageSize("65536\n"), 123), { group: 42, rss: 131072 });
  assert.deepEqual(parseProcessStat(stat({ name: "" }), 4096, 123), { group: 42, rss: 8192 });
  for (const text of ["", "0", "-1", "unknown", "4096 extra", "65535", "2097152"]) {
    assert.throws(() => parsePageSize(text), /unobservable/u);
  }
});

test("single-record exit accounting never combines old stat state with newer status", async () => {
  const reads = [];
  const result = await processGroupUsage(42, {
    pageSize: 4096, list: async () => ["123"],
    async read(path) {
      reads.push(path);
      assert.equal(path, "/proc/123/stat");
      return stat({ state: "R", pages: "0" });
    },
  });
  assert.deepEqual(result, { rss: 0, count: 1 });
  assert.deepEqual(reads, ["/proc/123/stat"]);
});

test("missing/malformed RSS for a live or zombie process never defaults to zero", () => {
  for (const state of ["R", "Z"]) {
    for (const pages of ["", "-1", "NaN", "2199023255552", "9007199254740992"]) {
      assert.throws(() => parseProcessStat(stat({ state, pages }), 4096, 123));
    }
  }
  assert.throws(() => parseProcessStat(stat(), 4096, 124), /identity changed/u);
  assert.throws(() => parseProcessStat(stat().replace(") R", ")R"), 4096, 123), /malformed/u);
});

test("explicitly disappeared tasks are distinct from permission and malformed-data failures", async () => {
  for (const code of ["ENOENT", "ESRCH"]) {
    const result = await processGroupUsage(42, {
      pageSize: 4096, list: async () => ["123"],
      read: async () => { throw Object.assign(new Error("task disappeared"), { code }); },
    });
    assert.deepEqual(result, { rss: 0, count: 0 });
  }
  await assert.rejects(processGroupUsage(42, {
    pageSize: 4096, list: async () => ["123"],
    read: async () => { throw Object.assign(new Error("unreadable task"), { code: "EACCES" }); },
  }), /unreadable/u);
});

test("only the requested process group's observed pages are accumulated", async () => {
  const result = await processGroupUsage(42, {
    pageSize: 4096, list: async () => ["123", "124", "self"],
    read: async (path) => path === "/proc/123/stat" ? stat() : stat({ pid: 124, group: 99, pages: "800" }),
  });
  assert.deepEqual(result, { rss: 8192, count: 1 });
});

test("detached groups and namespace orphans remain in the check's descendant accounting", async () => {
  for (const group of [42, 124]) {
    const records = new Map([
      ["123", stat()],
      ["124", stat({ pid: 124, parent: 123, group, started: 2, pages: "3", name: "namespace init" })],
      ["125", stat({ pid: 125, parent: 124, group: 125, started: 3, pages: String(75 * MiB / 4096) })],
      ["126", stat({ pid: 126, parent: 124, group: 126, started: 4, pages: "5" })],
      ["888", stat({ pid: 888, parent: 1, group: 888, pages: "10000" })],
    ]);
    const result = await processGroupUsage(42, {
      pageSize: 4096, list: async () => [...records.keys()].reverse(),
      read: async (path) => records.get(path.split("/")[2]),
    });
    assert.deepEqual(result, { rss: 75 * MiB + 10 * 4096, count: 4 });
    assert.match(runningProblem({ ...result, output: 0, scratch: 0 },
      { ...BOUNDS, maxRss: 64 * MiB }), /memory ceiling/u);
  }
});

test("missing ancestry counters and a reused parent identity cannot hide live usage", async () => {
  for (const change of [{ parent: "" }, { parent: "-1" }, { started: "" }, { started: "-1" }]) {
    assert.throws(() => parseProcessStat(stat(change), 4096, 123), /unobservable|truncated/u);
  }
  await assert.rejects(processGroupUsage(42, {
    pageSize: 4096, list: async () => ["123", "124"],
    read: async (path) => path === "/proc/123/stat" ? stat({ started: 10 })
      : stat({ pid: 124, parent: 123, group: 124, started: 2 }),
  }), /ancestry identity changed/u);
});

test("the process observation table is bounded before reading any records", async () => {
  let reads = 0;
  await assert.rejects(processGroupUsage(42, {
    pageSize: 4096, list: async () => Array(8193).fill("123"),
    read: async () => { reads += 1; return stat(); },
  }), /observation bound/u);
  assert.equal(reads, 0);
});
