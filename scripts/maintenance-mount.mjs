import { open, statfs } from "node:fs/promises";

import { requireValue } from "./maintenance-contract.mjs";
import { readBoundedFile } from "./maintenance-files.mjs";

const PATH_FLAGS = 0o10000000 | 0o400000; // Linux O_PATH | O_NOFOLLOW.

function mountPath(value) {
  requireValue(!/\\(?!040|011|012|134)/u.test(value), "malformed mount path escape");
  const path = value.replace(/\\(040|011|012|134)/gu,
    (_, octal) => String.fromCharCode(Number.parseInt(octal, 8)));
  requireValue(path.startsWith("/"), "mount path is not absolute");
  return path;
}

function positiveId(value) {
  const id = Number(value);
  requireValue(/^[1-9]\d*$/u.test(value) && Number.isSafeInteger(id), "mount identity is unobservable");
  return id;
}

export function descriptorMountId(text) {
  const lines = text.split("\n").filter((line) => line.startsWith("mnt_id:"));
  requireValue(lines.length === 1, "descriptor mount identity is missing or duplicated");
  const match = /^mnt_id:[ \t]+([1-9]\d*)$/u.exec(lines[0]);
  requireValue(match, "descriptor mount identity is malformed");
  return positiveId(match[1]);
}

export function readOnlyMount(mountinfo, path, mountId) {
  requireValue(Number.isSafeInteger(mountId) && mountId > 0, "effective descriptor mount ID is required");
  const mounts = new Map();
  for (const line of mountinfo.trim().split("\n")) {
    const parts = line.split(" - ");
    const fields = parts[0].split(" ");
    requireValue(parts.length === 2 && fields.length >= 6
      && parts[1].split(" ").length === 3, "malformed mountinfo record");
    const id = positiveId(fields[0]);
    requireValue(!mounts.has(id), "duplicate mount identity");
    const options = fields[5].split(",");
    requireValue(options.includes("ro") !== options.includes("rw"), "mount writability is unobservable");
    mounts.set(id, { path: mountPath(fields[4]), readOnly: options.includes("ro") });
  }
  const mount = mounts.get(mountId);
  requireValue(mount, "effective mount is unobservable");
  requireValue(path === mount.path || path.startsWith(mount.path === "/" ? "/" : `${mount.path}/`),
    "descriptor mount does not contain the requested path");
  return mount.readOnly;
}

async function readText(path, maximum) {
  return new TextDecoder("utf-8", { fatal: true }).decode(await readBoundedFile(path, maximum));
}

export async function filesystemSnapshot(path, {
  openPath = open, read = readText, capacity = statfs,
} = {}) {
  const handle = await openPath(path, PATH_FLAGS);
  try {
    requireValue(Number.isSafeInteger(handle.fd) && handle.fd >= 0, "path descriptor is unobservable");
    const observations = await Promise.allSettled([
      handle.stat({ bigint: true }),
      read(`/proc/self/fdinfo/${handle.fd}`, 8192),
      read("/proc/self/mountinfo", 1024 * 1024),
      capacity(`/proc/self/fd/${handle.fd}`, { bigint: true }),
    ]);
    const failure = observations.find((observed) => observed.status === "rejected");
    if (failure) throw failure.reason;
    const [metadata, info, mounts, fs] = observations.map((observed) => observed.value);
    requireValue(metadata.isDirectory() && !metadata.isSymbolicLink(),
      "capacity path must be an existing non-symlink directory");
    requireValue(typeof metadata.dev === "bigint" && metadata.dev >= 0n,
      "filesystem device identity is unobservable");
    requireValue(typeof fs.bavail === "bigint" && fs.bavail >= 0n
      && typeof fs.bsize === "bigint" && fs.bsize > 0n
      && typeof fs.type === "bigint", "filesystem capacity is unobservable");
    const bytes = fs.bavail * fs.bsize;
    requireValue(bytes <= BigInt(Number.MAX_SAFE_INTEGER)
      && Number.isSafeInteger(Number(fs.type)), "filesystem capacity exceeds the safe numeric range");
    const mountId = descriptorMountId(info);
    return { path, free: Number(bytes), type: Number(fs.type), device: String(metadata.dev),
      mountId, readOnly: readOnlyMount(mounts, path, mountId) };
  } finally {
    await handle.close();
  }
}
