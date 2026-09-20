import { posix } from "node:path";

import { requireValue } from "./maintenance-contract.mjs";

export const RUST_IDENTITY = "BUREAU_RUST_IDENTITY";
export const IDENTITY_MAX_BYTES = 32 * 1024;
const SCHEMA = "bureau-rust-identity-v1";
const MAX_ENTRIES = 128;
const DECIMAL = /^(?:0|[1-9]\d{0,39})$/u;

function decode(text) {
  requireValue(typeof text === "string" && text.length > 0
    && Buffer.byteLength(text) <= IDENTITY_MAX_BYTES, "missing or oversized Rust identity receipt");
  const value = JSON.parse(text);
  requireValue(value && Object.keys(value).length === 2 && value.schema === SCHEMA
    && Array.isArray(value.entries) && value.entries.length > 0 && value.entries.length <= MAX_ENTRIES,
  "malformed Rust identity receipt");
  const entries = new Map();
  for (const entry of value.entries) {
    requireValue(Array.isArray(entry) && entry.length === 4, "malformed Rust identity entry");
    const [path, ...identity] = entry;
    requireValue(typeof path === "string" && path.length <= 1024 && posix.isAbsolute(path)
      && posix.normalize(path) === path && !/[\s:\0]/u.test(path) && !entries.has(path)
      && identity.every((field) => typeof field === "string" && DECIMAL.test(field)),
    "malformed or duplicated Rust identity path");
    entries.set(path, identity.join(":"));
  }
  return entries;
}

function identityFields(metadata) {
  return ["dev", "ino", "ctimeNs"].map((key) => {
    const value = metadata[key];
    requireValue(typeof value === "bigint" && value >= 0n && DECIMAL.test(String(value)),
      `Rust descriptor ${key} identity is unobservable`);
    return String(value);
  });
}

export async function rustIdentity(environment, rootOwner, read) {
  if (rootOwner) {
    const mapping = (await read("/proc/self/uid_map", 8192)).toString("utf8");
    requireValue(/^\s*0\s+0\s+4294967295\s*$/u.test(mapping),
      "Rust admission must run outside a remapped user namespace");
  }
  const expected = rootOwner ? null : decode(environment[RUST_IDENTITY]);
  const observed = new Map();
  return {
    observe(path, metadata) {
      const fields = identityFields(metadata);
      const identity = fields.join(":");
      requireValue(rootOwner ? metadata.uid === 0n : expected.get(path) === identity,
        `Rust runtime ${rootOwner ? "host-root ownership" : "identity receipt"} differs: ${path}`);
      requireValue(!observed.has(path) || observed.get(path).join(":") === identity,
        `Rust runtime changed during admission: ${path}`);
      observed.set(path, fields);
      requireValue(observed.size <= MAX_ENTRIES, "Rust identity receipt has too many paths");
    },
    finish() {
      requireValue(observed.size > 0 && (!expected || observed.size === expected.size),
        "Rust identity receipt contains missing or unexpected paths");
      const entries = [...observed].sort(([left], [right]) => left < right ? -1 : Number(left > right))
        .map(([path, fields]) => [path, ...fields]);
      const text = JSON.stringify({ schema: SCHEMA, entries });
      requireValue(Buffer.byteLength(text) <= IDENTITY_MAX_BYTES, "oversized Rust identity receipt");
      return text;
    },
  };
}
