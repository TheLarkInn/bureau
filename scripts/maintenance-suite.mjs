import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, realpath, writeFile } from "node:fs/promises";
import { isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { SHA, requireValue } from "./maintenance-contract.mjs";
import { CHAOS_TEST, workspace } from "./maintenance-checks.mjs";
import { readBoundedJson } from "./maintenance-files.mjs";

export async function binaryDigest(binary) {
  const info = await lstat(binary);
  requireValue(info.isFile() && !info.isSymbolicLink() && info.size <= 512 * 1024 * 1024,
    "suite binary must be a regular file no larger than 512 MiB");
  requireValue(process.platform === "linux" && (info.mode & 0o111) !== 0, "suite binary must be Linux-executable");
  const hash = createHash("sha256");
  let size = 0;
  for await (const bytes of createReadStream(binary)) {
    size += bytes.length;
    requireValue(size <= info.size, "suite binary grew while hashing");
    hash.update(bytes);
  }
  requireValue(size === info.size, "suite binary shrank while hashing");
  return hash.digest("hex");
}

export function validateSuite(suite) {
  requireValue(suite?.schema === "bureau-chaos-suite-v1" && SHA.test(suite.source_commit),
    "suite must pin a full source commit");
  requireValue(typeof suite.binary === "string" && isAbsolute(suite.binary),
    "suite must select an absolute prebuilt binary path");
  requireValue(/^[a-f0-9]{64}$/u.test(suite.sha256 ?? ""), "suite must pin the binary SHA-256");
  requireValue(suite.test === CHAOS_TEST, "suite must select the exact offline invariant test");
  return suite;
}

export async function readSuite(path) {
  const suite = validateSuite(await readBoundedJson(path));
  requireValue(await realpath(suite.binary) === suite.binary, "suite binary path must be canonical");
  requireValue(await binaryDigest(suite.binary) === suite.sha256, "prebuilt suite binary changed");
  return suite;
}

async function main() {
  const { values } = parseArgs({ options: { binary: { type: "string" }, output: { type: "string" } } });
  requireValue(values.binary && values.output, "usage: maintenance-suite.mjs --binary PATH --output PATH");
  const state = workspace();
  requireValue(!state.status, "record the suite only after a clean, source-pinned offline build");
  const binary = await realpath(values.binary);
  const suite = { schema: "bureau-chaos-suite-v1", source_commit: state.commit,
    binary, sha256: await binaryDigest(binary), test: CHAOS_TEST };
  await writeFile(values.output, `${JSON.stringify(suite, null, 2)}\n`, { flag: "wx", mode: 0o600 });
  console.log(JSON.stringify(suite));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
