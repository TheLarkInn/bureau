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

export function parseProcessStat(text, pageSize, expectedPid) {
  const opening = /^([1-9]\d*) \(/u.exec(text);
  const closing = text.lastIndexOf(")");
  requireValue(opening && closing >= opening[0].length && text[closing + 1] === " ",
    "malformed process stat record");
  requireValue(integer(opening[1], "pid") === expectedPid, "process stat identity changed");
  const fields = text.slice(closing + 1).trim().split(/\s+/u);
  requireValue(fields.length >= 22 && /^[a-zA-Z]$/u.test(fields[0]), "truncated process stat record");
  const group = integer(fields[2], "group");
  const rss = integer(fields[21], "RSS pages") * parsePageSize(String(pageSize));
  requireValue(Number.isSafeInteger(rss), "process RSS is outside the safe range");
  return { group, rss };
}

export async function processGroupUsage(pgid, {
  read = readFile, list = readdir, pageSize = systemPageSize(),
} = {}) {
  requireValue(Number.isSafeInteger(pgid) && pgid > 0, "invalid process group");
  let rss = 0;
  let count = 0;
  for (const name of await list("/proc")) {
    if (!/^[1-9]\d*$/u.test(name)) continue;
    let text;
    try {
      text = await read(`/proc/${name}/stat`, "utf8");
    } catch (error) {
      if (error.code === "ENOENT" || error.code === "ESRCH") continue;
      throw error;
    }
    // Group and resident pages must come from one record, not an earlier
    // stat state combined with a later status after exit_mm released memory.
    const observed = parseProcessStat(text, pageSize, Number(name));
    if (observed.group !== pgid) continue;
    count += 1;
    rss += observed.rss;
    requireValue(Number.isSafeInteger(rss), "process-group RSS is outside the safe range");
  }
  return { rss, count };
}
