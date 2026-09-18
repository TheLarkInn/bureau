import assert from "node:assert/strict";
import { join, posix, resolve } from "node:path";
import test from "node:test";

import { requirePreparedRust, rustEnvironment, validateRustPolicy } from "./maintenance-rust.mjs";
import { RUST_IDENTITY } from "./maintenance-rust-identity.mjs";
import { POLICY } from "./maintenance-test-support.mjs";

const ROOT = resolve("runtime-fixture");
const STABLE = "1.94.0";
const NIGHTLY = "nightly-2026-01-22";
const sysroot = (channel) => `${POLICY.rustup_home}/toolchains/${channel}-${POLICY.rust_host}`;
const DRIVER = `${POLICY.dylint_drivers}/${NIGHTLY}-${POLICY.rust_host}/dylint-driver`;
const RUSTC_DRIVER = `${sysroot(NIGHTLY)}/lib/rustlib/${POLICY.rust_host}/lib/librustc_driver-1234abcd.so`;
const ENVIRONMENT = {
  HOME: posix.dirname(POLICY.dylint_drivers),
  PATH: `${POLICY.rust_bin}:/opt/bureau/bin:/usr/local/bin:/usr/bin:/bin`,
  RUSTUP_HOME: POLICY.rustup_home, CARGO_HOME: POLICY.cargo_home,
  CARGO_NET_OFFLINE: "true", RUSTUP_AUTO_INSTALL: "0",
};

function inventory() {
  const entries = new Map();
  const calls = [];
  function add(path, kind = "directory") {
    if (entries.has(path)) return;
    if (path !== "/") add(posix.dirname(path));
    entries.set(path, { kind, uid: 0, mode: kind === "file" ? 0o555 : 0o755, readOnly: true,
      dev: 9007199254740993n, ino: BigInt(entries.size) + 9007199254740993n, ctimeNs: 1234567890123456789n });
  }
  for (const path of [POLICY.cargo_home, POLICY.rustup_home, POLICY.rust_bin, POLICY.dylint_drivers]) add(path);
  for (const command of ["rustup", "cargo-dylint", "dylint-link"]) add(`${POLICY.rust_bin}/${command}`, "file");
  for (const command of ["cargo", "rustc", "rustdoc", "rustfmt", "cargo-fmt", "clippy-driver", "cargo-clippy"]) {
    add(`${POLICY.rust_bin}/${command}`, "symlink");
    entries.get(`${POLICY.rust_bin}/${command}`).target = `${POLICY.rust_bin}/rustup`;
    add(`${sysroot(STABLE)}/bin/${command}`, "file");
  }
  for (const command of ["cargo", "rustc", "rustdoc"]) add(`${sysroot(NIGHTLY)}/bin/${command}`, "file");
  for (const channel of [STABLE, NIGHTLY]) add(`${sysroot(channel)}/lib`);
  for (const name of ["registry", "git"]) add(`${POLICY.cargo_home}/${name}`);
  add(DRIVER, "file");
  add(RUSTC_DRIVER, "file");
  const pins = new Map([
    ["/proc/self/uid_map", "0 0 4294967295\n"],
    [join(ROOT, "rust-toolchain.toml"), `[toolchain]\nchannel = "${STABLE}"\n`],
    [join(ROOT, "lints/rust-lints/rust-toolchain"), `[toolchain]\nchannel = "${NIGHTLY}"\n`],
  ]);
  function entry(path) {
    assert.equal(entries.has(path), true, `missing provisioned path: ${path}`);
    return entries.get(path);
  }
  const inspect = {
    async realpath(path) { return entry(path).target ?? path; },
    async descriptorSnapshot(path) {
      const value = entry(path);
      return { readOnly: value.readOnly, metadata: { ...value, uid: BigInt(value.uid), mode: BigInt(value.mode),
        isDirectory: () => value.kind === "directory",
        isFile: () => value.kind === "file", isSymbolicLink: () => value.kind === "symlink" } };
    },
    async readdir(path) {
      entry(path);
      return [...entries.keys()].filter((key) => key !== path && posix.dirname(key) === path)
        .map((key) => posix.basename(key));
    },
    async read(path) {
      assert.equal(pins.has(path), true, `missing source pin: ${path}`);
      return Buffer.from(pins.get(path));
    },
  };
  function execute(command, args, options) {
    calls.push({ command, args, ...options });
    if (posix.basename(command) === "rustc") {
      return sysroot(args[0] === `+${NIGHTLY}` || options.cwd === join(ROOT, "lints", "rust-lints") ? NIGHTLY : STABLE);
    }
    if (args[0] === "--version") return `cargo ${STABLE} (offline fixture)`;
    if (args[0] === "dylint") return "cargo-dylint 5.0.0";
    assert.equal(command, DRIVER);
    assert.deepEqual(args, ["-V"]);
    return "dylint-driver 5.0.0";
  }
  return { entries, calls, pins, add, inspect, execute, environment: ENVIRONMENT, rootOwner: true };
}

test("Rust policy rejects omitted, malformed, overlapping and writable-output roots", () => {
  validateRustPolicy(POLICY);
  for (const change of [
    { cargo_home: undefined }, { rustup_home: "../rust" }, { rust_bin: "/opt/../bin" },
    { rust_bin: "/opt/bin:./hooks" }, { rust_bin: "/" }, { rust_bin: "/opt/bin/" },
    { cargo_home: POLICY.cargo_target }, { cargo_home: `${POLICY.cargo_target}/dependencies` },
    { cargo_home: "/cache" }, { rust_bin: POLICY.rustup_home }, { cargo_home: `${POLICY.rustup_home}/cargo` },
    { rust_host: "stable" }, { dylint_drivers: "/cache/drivers" },
  ]) assert.throws(() => validateRustPolicy({ ...POLICY, ...change }));
});

test("the exact runtime environment survives without credentials or ambient compiler hooks", () => {
  const env = rustEnvironment(POLICY, { ...ENVIRONMENT, GH_TOKEN: "not-authorized", COPILOT_HOME: "/private" });
  assert.deepEqual(env, { ...ENVIRONMENT, LANG: "C.UTF-8", CARGO_PROFILE_DEV_DEBUG: "0", CARGO_PROFILE_TEST_DEBUG: "0" });
  for (const key of Object.keys(ENVIRONMENT)) {
    const missing = { ...ENVIRONMENT };
    delete missing[key];
    assert.throws(() => rustEnvironment(POLICY, missing), new RegExp(key, "u"));
    assert.throws(() => rustEnvironment(POLICY, { ...ENVIRONMENT, [key]: "incorrect" }), new RegExp(key, "u"));
  }
  for (const key of ["RUSTUP_TOOLCHAIN", "RUSTUP_DIST_SERVER", "RUSTC_WRAPPER", "RUSTFLAGS",
    "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_REGISTRIES_CRATES_IO_TOKEN", "DYLINT_DRIVER_PATH",
    "DYLINT_LIBRARY_PATH", "LD_PRELOAD", "LD_LIBRARY_PATH", "NODE_OPTIONS", "BASH_ENV"]) {
    assert.throws(() => rustEnvironment(POLICY, { ...ENVIRONMENT, [key]: "unreviewed" }), /override/u);
  }
});

test("prepared tools select repository stable, explicit nightly and lint-directory nightly through real proxies", async () => {
  const fixture = inventory();
  const { [RUST_IDENTITY]: receipt, ...homes } = await requirePreparedRust(ROOT, POLICY, fixture);
  assert.deepEqual(homes, { CARGO_HOME: POLICY.cargo_home, RUSTUP_HOME: POLICY.rustup_home });
  assert.equal(JSON.parse(receipt).schema, "bureau-rust-identity-v1");
  assert.deepEqual(fixture.calls.slice(0, 3).map(({ args, cwd }) => [args, cwd]), [
    [["--print", "sysroot"], ROOT],
    [[`+${NIGHTLY}`, "--print", "sysroot"], ROOT],
    [["--print", "sysroot"], join(ROOT, "lints", "rust-lints")],
  ]);
  for (const { env } of fixture.calls) assert.deepEqual(env, rustEnvironment(POLICY, ENVIRONMENT));
  assert.equal(fixture.calls.length, 6);
});

test("absent commands, offline dependencies, rustc-dev or prepared dylint driver fail before execution", async () => {
  for (const path of [POLICY.cargo_home, `${POLICY.cargo_home}/registry`, `${POLICY.cargo_home}/git`,
    `${POLICY.rust_bin}/rustup`, `${POLICY.rust_bin}/cargo`, `${POLICY.rust_bin}/cargo-dylint`,
    `${POLICY.rust_bin}/dylint-link`, `${sysroot(STABLE)}/bin/rustc`, RUSTC_DRIVER, DRIVER]) {
    const fixture = inventory();
    fixture.entries.delete(path);
    await assert.rejects(requirePreparedRust(ROOT, POLICY, fixture));
    assert.equal(fixture.calls.length, 0);
  }
});

test("mutable mounts, replaceable ancestors and redirected binaries cannot qualify", async () => {
  for (const [path, change] of [
    [POLICY.rustup_home, { readOnly: false }],
    [DRIVER, { readOnly: false }],
    [`${POLICY.cargo_home}/registry`, { readOnly: false }],
    [`${sysroot(NIGHTLY)}/bin`, { readOnly: false }],
    [posix.dirname(DRIVER), { readOnly: false }],
    ["/opt/bureau", { uid: 1000 }], ["/opt/bureau", { mode: 0o775 }],
    [POLICY.dylint_drivers, { uid: 1000 }], [DRIVER, { mode: 0o755 }],
    [POLICY.cargo_home, { target: "/another/cache" }],
    [`${POLICY.rust_bin}/cargo`, { kind: "file" }],
    [`${POLICY.rust_bin}/rustc`, { target: `${sysroot(STABLE)}/bin/rustc` }],
  ]) {
    const fixture = inventory();
    Object.assign(fixture.entries.get(path), change);
    await assert.rejects(requirePreparedRust(ROOT, POLICY, fixture), /ownership|protected|read-only|immutable|canonical|proxy/u);
    assert.equal(fixture.calls.length, 0);
  }
});

test("namespace checks bind the exact host-root admission rather than accepting unmapped ownership", async () => {
  const fixture = inventory();
  const prepared = await requirePreparedRust(ROOT, POLICY, fixture);
  for (const value of fixture.entries.values()) value.uid = 65534;
  fixture.pins.set("/proc/self/uid_map", "0 1000 1\n");
  fixture.environment = { ...ENVIRONMENT, [RUST_IDENTITY]: prepared[RUST_IDENTITY] };
  fixture.rootOwner = false;
  await requirePreparedRust(ROOT, POLICY, fixture);
  await assert.rejects(requirePreparedRust(ROOT, POLICY, { ...fixture, rootOwner: true }), /remapped user namespace/u);
  fixture.entries.get(DRIVER).ino += 1n;
  await assert.rejects(requirePreparedRust(ROOT, POLICY, fixture), /identity receipt differs/u);
});

test("missing, stale and extra identity entries fail before any runtime command", async () => {
  for (const change of [
    () => undefined,
    (value) => ({ ...value, entries: value.entries.slice(1) }),
    (value) => ({ ...value, entries: [...value.entries, ["/unexpected", "1", "2", "3"]] }),
    (value) => ({ ...value, entries: value.entries.map(([path, dev, ino, ctime]) =>
      [path, dev, ino, String(BigInt(ctime) + 1n)]) }),
  ]) {
    const fixture = inventory();
    const prepared = await requirePreparedRust(ROOT, POLICY, fixture);
    const receipt = JSON.stringify(change(JSON.parse(prepared[RUST_IDENTITY])));
    fixture.calls.length = 0;
    await assert.rejects(requirePreparedRust(ROOT, POLICY, {
      ...fixture, rootOwner: false, environment: { ...ENVIRONMENT, [RUST_IDENTITY]: receipt },
    }), /identity receipt/u);
    assert.equal(fixture.calls.length, 0);
  }
});

test("prepared Cargo homes cannot smuggle credentials or compiler configuration", async () => {
  for (const name of ["credentials", "credentials.toml", "config", "config.toml"]) {
    const fixture = inventory();
    fixture.add(`${POLICY.cargo_home}/${name}`, "file");
    await assert.rejects(requirePreparedRust(ROOT, POLICY, fixture), /credentials or configuration/u);
    assert.equal(fixture.calls.length, 0);
  }
});

test("malformed pins and mismatched command selections or driver versions fail explicitly", async () => {
  const malformed = inventory();
  malformed.pins.set(join(ROOT, "rust-toolchain.toml"), '[toolchain]\nchannel = "stable"\n');
  await assert.rejects(requirePreparedRust(ROOT, POLICY, malformed), /toolchain pin/u);
  for (const command of ["rustc", "cargo", "dylint-driver"]) {
    const fixture = inventory();
    const execute = (path, args, options) => posix.basename(path) === command
      ? "unqualified version" : fixture.execute(path, args, options);
    await assert.rejects(requirePreparedRust(ROOT, POLICY, { ...fixture, execute }), /toolchain|version|immutable nightly driver/u);
  }
  const failed = inventory();
  await assert.rejects(requirePreparedRust(ROOT, POLICY, {
    ...failed, execute() { throw new Error("missing offline tool"); },
  }), /missing offline tool/u);
});

test("the Dylint driver must load directly without rustup injecting loader paths", async () => {
  const fixture = inventory();
  const execute = (command, args, options) => {
    if (command === DRIVER) throw new Error("missing canonical rustc_driver library");
    return fixture.execute(command, args, options);
  };
  await assert.rejects(requirePreparedRust(ROOT, POLICY, { ...fixture, execute }),
    /missing canonical rustc_driver library/u);
});
