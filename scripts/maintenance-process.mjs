import { execFileSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";

import { requireValue } from "./maintenance-contract.mjs";

export function parsePageSize(text) {
  const trimmed = text.trim();
  const size = Number(trimmed);
  requireValue(/^[1-9]\d*$/u.test(trimmed) && Number.isSafeInteger(size)
    && size <= 1024 * 1024 && (size & (size - 1)) === 0, "Linux page size is unobservable");
  return size;
}

let observedPageSize;

function systemPageSize() {
  observedPageSize ??= parsePageSize(execFileSync("getconf", ["PAGESIZE"], {
    encoding: "utf8", timeout: 1000, maxBuffer: 256, env: { PATH: "/usr/bin:/bin", LC_ALL: "C" },
  }));
  return observedPageSize;
}

function integer(value, field) {
  const number = Number(value);
  requireValue(/^\d+$/u.test(value ?? "") && Number.isSafeInteger(number),
    `process ${field} is unobservable`);
  return number;
}

function processRecord(text, pageSize, expectedPid) {
  const opening = /^([1-9]\d*) \(/u.exec(text);
  const closing = text.lastIndexOf(")");
  requireValue(opening && closing >= opening[0].length && text[closing + 1] === " ",
    "malformed process stat record");
  const pid = integer(opening[1], "pid");
  requireValue(pid === expectedPid, "process stat identity changed");
  const fields = text.slice(closing + 1).trim().split(/\s+/u);
  requireValue(fields.length >= 22 && /^[a-zA-Z]$/u.test(fields[0]), "truncated process stat record");
  const parent = integer(fields[1], "parent");
  const group = integer(fields[2], "group");
  const started = integer(fields[19], "start time");
  const rss = integer(fields[21], "RSS pages") * parsePageSize(String(pageSize));
  requireValue(Number.isSafeInteger(rss), "process RSS is outside the safe range");
  return { pid, parent, group, started, rss };
}

export function parseProcessStat(text, pageSize, expectedPid) {
  const { group, rss } = processRecord(text, pageSize, expectedPid);
  return { group, rss };
}

function treeUsage(records, pgid) {
  const children = new Map();
  const pending = [];
  for (const record of records) {
    if (!children.has(record.parent)) children.set(record.parent, []);
    children.get(record.parent).push(record);
    if (record.group === pgid) pending.push(record);
  }
  const seen = new Set();
  let rss = 0;
  for (let index = 0; index < pending.length; index += 1) {
    const record = pending[index];
    if (seen.has(record.pid)) continue;
    seen.add(record.pid);
    rss += record.rss;
    requireValue(Number.isSafeInteger(rss), "process-group RSS is outside the safe range");
    for (const child of children.get(record.pid) ?? []) {
      requireValue(child.started >= record.started, "process ancestry identity changed");
      pending.push(child);
    }
  }
  return { rss, count: seen.size };
}

export async function processGroupUsage(pgid, {
  read = readFile, list = readdir, pageSize = systemPageSize(),
} = {}) {
  requireValue(Number.isSafeInteger(pgid) && pgid > 0, "invalid process group");
  const names = await list("/proc");
  requireValue(names.length <= 8192, "process table exceeds the observation bound");
  const records = [];
  for (const name of names) {
    if (!/^[1-9]\d*$/u.test(name)) continue;
    let text;
    try {
      text = await read(`/proc/${name}/stat`, "utf8");
    } catch (error) {
      if (error.code === "ENOENT" || error.code === "ESRCH") continue;
      throw error;
    }
    // Ancestry, identity and RSS belong to one record, including during exit.
    records.push(processRecord(text, pageSize, Number(name)));
  }
  // setsid changes a descendant's group, not its parent chain. Namespace
  // orphans are reparented to the child namespace's init within this tree.
  return treeUsage(records, pgid);
}
