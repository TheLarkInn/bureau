import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { ENGINE, GUARD, parseProperties } from "../deployment/supervision/system.mjs";
import { OWNER_PATH, PROFILE_PROPERTIES, REPORTER_ENV, reporterMetadata,
  serviceProfile } from "../deployment/supervision/profile.mjs";
import { binaryApproval, binaryDigest, interpreterApproval } from "../deployment/supervision/provenance.mjs";
import { unitArguments } from "../deployment/supervision/transaction.mjs";
import { NATIVE_BUDGET } from "../deployment/supervision/budget.mjs";
import { COMMIT } from "./supervision-test-support.mjs";

const unit = `/etc/systemd/system/${ENGINE}`;
const reporter = `${unit}.d/20-reporter-credential.conf`;
const supervision = `${unit}.d/windows-supervision.conf`;
const effective = () => ({
  FragmentPath: unit, DropInPaths: `${reporter} ${supervision}`,
  EnvironmentFiles: `/etc/bureau/maintenance.env (ignore_errors=no) ${REPORTER_ENV} (ignore_errors=no)`,
  NeedDaemonReload: "no", RefuseManualStart: "yes", Restart: "no", BindsTo: GUARD,
  After: `network-online.target ${GUARD}`, TimeoutStopUSec: "5s", KillMode: "mixed",
  SendSIGKILL: "yes", User: "bureau", MemoryMax: String(NATIVE_BUDGET.engineMemoryMaxBytes),
});

test("only the paired reviewed drop-ins and required reporter EnvironmentFile are admitted", () => {
  serviceProfile(effective(), ENGINE, GUARD);
  for (const change of [{ DropInPaths: supervision }, { DropInPaths: reporter },
    { DropInPaths: `${reporter} ${supervision} ${unit}.d/99-override.conf` },
    { DropInPaths: [reporter, supervision] }, { EnvironmentFiles: `/etc/bureau/maintenance.env (ignore_errors=no)` },
    { EnvironmentFiles: `${effective().EnvironmentFiles} /tmp/override (ignore_errors=no)` },
    { EnvironmentFiles: effective().EnvironmentFiles.replaceAll("ignore_errors=no", "ignore_errors=yes") },
    { Restart: "on-failure" }, { NeedDaemonReload: "yes" }, { RefuseManualStart: "no" }, { MemoryMax: "infinity" }]) {
    assert.throws(() => serviceProfile({ ...effective(), ...change }, ENGINE, GUARD), /binding/u);
  }
});

test("repeated EnvironmentFiles rows preserve the required service profile", () => {
  const rows = Object.entries(effective()).flatMap(([name, value]) => name === "EnvironmentFiles"
    ? ["EnvironmentFiles=/etc/bureau/maintenance.env (ignore_errors=no)",
      `EnvironmentFiles=${REPORTER_ENV} (ignore_errors=no)`]
    : [`${name}=${value}`]);
  const state = parseProperties(rows.join("\n"), PROFILE_PROPERTIES);
  serviceProfile(state, ENGINE, GUARD);
  assert.equal(state.EnvironmentFiles, effective().EnvironmentFiles);
});

test("repeated scalar systemd properties remain invalid", () => {
  assert.throws(() => parseProperties([
    "FragmentPath=/etc/systemd/system/bureau-maintenance.service",
    "FragmentPath=/etc/systemd/system/other.service",
    "EnvironmentFiles=/etc/bureau/maintenance.env (ignore_errors=no)",
  ].join("\n"), ["FragmentPath", "EnvironmentFiles"]), /incomplete systemd observation/u);
});

test("unknown and incomplete systemd properties remain invalid", () => {
  for (const text of ["FragmentPath=/etc/systemd/system/bureau-maintenance.service",
    "FragmentPath=/etc/systemd/system/bureau-maintenance.service\nUnknown=value\nEnvironmentFiles=/etc/bureau"]) {
    assert.throws(() => parseProperties(text, ["FragmentPath", "EnvironmentFiles"]),
      /incomplete systemd observation/u);
  }
});

test("shared startup allocation cannot drift from the reviewed engine and heartbeat bounds", async () => {
  const service = await readFile(new URL("../deployment/bureau-maintenance.service", import.meta.url), "utf8");
  const memory = /^MemoryMax=(\d+)G$/mu.exec(service);
  assert.equal(Number(memory[1]) * 1024 ** 3, NATIVE_BUDGET.engineMemoryMaxBytes);
  assert.equal(unitArguments().includes(`--property=MemoryMax=${NATIVE_BUDGET.guardianMemoryMaxBytes}`), true);
  assert.equal(NATIVE_BUDGET.engineMemoryMaxBytes + NATIVE_BUDGET.guardianMemoryMaxBytes + 1024 ** 3, 9797894144);
});

test("reporter template preserves the exact approved non-secret 86-byte binding", async () => {
  const bytes = await readFile(new URL("../deployment/20-reporter-credential.conf", import.meta.url));
  assert.equal(bytes.length, 86);
  assert.equal(createHash("sha256").update(bytes).digest("hex"),
    "d41474b01f3e034cd2f2ffdb8e9b0556c5e6e43566b9f8347ceaceebb2bf7db6");
});

test("required reporter metadata is checked without reading credential contents", () => {
  const metadata = { uid: 0, gid: 0, mode: 0o100600, size: 100,
    isFile: () => true, isSymbolicLink: () => false };
  reporterMetadata(metadata, 1001, 1001);
  for (const change of [{ mode: 0o100644 }, { mode: 0o100640 }, { mode: undefined }, { uid: 1001 },
    { size: 0 }, { size: 8193 }, { isFile: () => false }, { isSymbolicLink: () => true }]) {
    assert.throws(() => reporterMetadata({ ...metadata, ...change }, 1001, 1001), /reporter/u);
  }
});

test("canonical Node 24 path, version and digest pins reject absent or mismatched interpreters", async () => {
  const value = { node_path: "/opt/bureau/bin/node", node_sha256: "a".repeat(64) };
  const current = { path: value.node_path, version: "24.0.2" };
  assert.equal(interpreterApproval(value, current), value.node_sha256);
  for (const update of [{ path: "/usr/bin/node" }, { version: "22.0.0" }, { version: undefined },
    { version: ["24.0.2"] }]) {
    assert.throws(() => interpreterApproval(value, { ...current, ...update }), /interpreter/u);
  }
  for (const update of [{ node_path: [value.node_path] }, { node_sha256: [value.node_sha256] },
    { node_path: "/opt/../usr/bin/node" }, { node_sha256: null }]) {
    assert.throws(() => interpreterApproval({ ...value, ...update }, current), /interpreter/u);
  }
  await assert.rejects(binaryDigest(new URL("./missing-supervision-interpreter", import.meta.url)), { code: "ENOENT" });
  assert.equal(unitArguments().includes(process.execPath), true);
});

test("all native provenance regex fields reject JSON type coercion", () => {
  const good = { schema: "bureau-native-supervision-v1", approved: true, commit: COMMIT,
    bureau_sha256: "b".repeat(64), node_path: "/opt/bureau/bin/node", node_sha256: "c".repeat(64) };
  for (const field of ["commit", "bureau_sha256"]) {
    for (const value of [[good[field]], null, {}, true, 123]) {
      assert.throws(() => binaryApproval({ ...good, [field]: value }, COMMIT), /provenance/u);
    }
  }
});

const ENTRY = "deployment/windows-entry.sh";
const ROOT = fileURLToPath(new URL("..", import.meta.url));

const indexedMode = (path) => {
  const listed = spawnSync("git", ["ls-files", "-s", "--", path], { cwd: ROOT, encoding: "utf8" });
  return listed.status === 0 ? listed.stdout.split(" ")[0] || undefined : undefined;
};

// wsl.exe --exec calls execvpe on the entry itself; a 0644 entry fails with Permission denied.
test("Windows supervision entry is executable for wsl.exe --exec", {
  skip: process.platform === "win32" && "Windows file modes do not carry owner-execute",
}, async () => {
  const { mode } = await stat(new URL(`../${ENTRY}`, import.meta.url));
  assert.deepEqual([mode & 0o100, indexedMode(ENTRY) ?? "100755"], [0o100, "100755"]);
});

test("native bootstrap and unprivileged claim use the original service PATH without root prefix", async () => {
  const files = await Promise.all(["bureau-maintenance.service", "windows-entry.sh", "windows-supervision.conf"]
    .map((name) => readFile(new URL(`../deployment/${name}`, import.meta.url), "utf8")));
  for (const file of files) assert.equal(file.includes(`PATH=${OWNER_PATH}`), true);
  assert.equal(files.slice(1).some((file) => file.includes("/usr/bin/node")), false);
  assert.match(files[2], /ExecStartPre=\/usr\/bin\/env -i /u);
  assert.equal(files[2].includes("ExecStartPre=+"), false);
});
