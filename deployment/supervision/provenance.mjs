import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { open } from "node:fs/promises";
import { posix } from "node:path";

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { readBoundedJson } from "../../scripts/maintenance-files.mjs";
import { descriptorSnapshot } from "../../scripts/maintenance-mount.mjs";
import { COMMIT, exactKeys } from "./protocol.mjs";

export function binaryApproval(value, commit) {
  exactKeys(value, ["schema", "approved", "commit", "bureau_sha256", "node_path", "node_sha256"]);
  requireValue(value.schema === "bureau-native-supervision-v1" && value.approved === true
    && typeof value.commit === "string" && COMMIT.test(value.commit) && value.commit === commit
    && typeof value.bureau_sha256 === "string" && /^[a-f0-9]{64}$/u.test(value.bureau_sha256),
  "native binary provenance is not approved");
  return value.bureau_sha256;
}

export function interpreterApproval(value, current = { path: process.execPath, version: process.versions.node }) {
  const { path, version } = current;
  requireValue(typeof value.node_path === "string" && /^\/(?:opt|usr)\/[a-zA-Z0-9_./-]+$/u.test(value.node_path)
    && posix.normalize(value.node_path) === value.node_path && value.node_path === path
    && typeof value.node_sha256 === "string" && /^[a-f0-9]{64}$/u.test(value.node_sha256)
    && typeof version === "string" && /^24\.\d+\.\d+$/u.test(version), "qualified canonical Node 24 interpreter differs");
  return value.node_sha256;
}

export async function binaryDigest(path) {
  const handle = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const info = await handle.stat({ bigint: true });
    requireValue(info.isFile() && info.size > 0n && info.size <= 128n * 1024n * 1024n,
      "binary provenance exceeds its byte bound");
    const hash = createHash("sha256");
    let bytes = 0n;
    for await (const chunk of handle.createReadStream({ autoClose: false, highWaterMark: 65_536 })) {
      bytes += BigInt(chunk.length);
      requireValue(bytes <= info.size, "binary changed while hashing");
      hash.update(chunk);
    }
    const after = await handle.stat({ bigint: true });
    requireValue(bytes === info.size && after.dev === info.dev && after.ino === info.ino
      && after.ctimeNs === info.ctimeNs && after.size === info.size, "binary changed while hashing");
    return hash.digest("hex");
  } finally {
    await handle.close();
  }
}

export async function binaryProvenance(commit, protect) {
  const approval = "/etc/bureau/windows-supervision.json";
  await protect(approval);
  const value = await readBoundedJson(approval);
  const binaries = [["/opt/bureau/bin/bureau", binaryApproval(value, commit)],
    [value.node_path, interpreterApproval(value)]];
  for (const [path, expected] of binaries) {
    await protect(path);
    const snapshot = await descriptorSnapshot(path);
    requireValue(snapshot.readOnly && snapshot.metadata.isFile() && (snapshot.metadata.mode & 0o222n) === 0n,
      "qualified executable must be immutable");
    requireValue(await binaryDigest(path) === expected, "executable differs from the approved provenance");
  }
}
