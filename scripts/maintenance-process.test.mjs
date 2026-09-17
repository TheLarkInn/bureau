import assert from "node:assert/strict";
import test from "node:test";

import { parsePageSize, parseProcessStat, processGroupUsage } from "./maintenance-process.mjs";

function stat({ pid = 123, group = 42, state = "R", pages = "2", name = "test (process)" } = {}) {
  const fields = Array(22).fill("0");
  fields[0] = state;
  fields[2] = String(group);
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
