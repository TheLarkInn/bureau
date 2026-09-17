import { constants } from "node:fs";
import { open } from "node:fs/promises";

import { requireValue } from "./maintenance-contract.mjs";

export async function readBoundedFile(path, maximum, openFile = open) {
  requireValue(Number.isSafeInteger(maximum) && maximum > 0 && maximum <= 1024 * 1024,
    "invalid bounded-file byte ceiling");
  const handle = await openFile(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const metadata = await handle.stat();
    requireValue(metadata.isFile() && metadata.size <= maximum, "file is not regular or exceeds the byte limit");
    const buffer = Buffer.alloc(maximum + 1);
    let used = 0;
    while (used <= maximum) {
      const { bytesRead } = await handle.read(buffer, used, buffer.length - used, used);
      if (!bytesRead) return buffer.subarray(0, used);
      used += bytesRead;
      requireValue(used <= maximum, "file grew beyond the byte limit while reading");
    }
    throw new Error("bounded-file reader exceeded its ceiling");
  } finally {
    await handle.close();
  }
}

export async function readBoundedJson(path, maximum = 8192, openFile = open) {
  const buffer = await readBoundedFile(path, maximum, openFile);
  return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(buffer));
}
