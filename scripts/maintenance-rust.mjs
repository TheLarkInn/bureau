import { execFileSync } from "node:child_process";
import { lstat, readdir, realpath } from "node:fs/promises";
import { join, posix } from "node:path";

import { childEnvironment } from "./maintenance-child.mjs";
import { requireValue } from "./maintenance-contract.mjs";
import { readBoundedFile } from "./maintenance-files.mjs";
import { filesystemSnapshot } from "./maintenance-mount.mjs";

export const RUST_PATHS = ["rust_bin", "rustup_home", "cargo_home", "dylint_drivers"];
export const DYLINT_VERSION = "5.0.0";
const PROXIES = ["cargo", "rustc", "rustdoc", "rustfmt", "cargo-fmt", "clippy-driver", "cargo-clippy"];
const INSPECT = { lstat, readdir, realpath, filesystemSnapshot, read: readBoundedFile, uid: () => process.getuid() };

export function validateRustPolicy(policy) {
  for (const key of RUST_PATHS) {
    const path = policy[key];
    requireValue(typeof path === "string" && posix.isAbsolute(path) && !path.endsWith("/")
      && posix.normalize(path) === path && !/[\s:]/u.test(path),
    `${key} must be an explicit canonical read-only Linux path`);
  }
  requireValue(/^(?:x86_64|aarch64)-unknown-linux-gnu$/u.test(policy.rust_host ?? ""),
    "rust_host must name the qualified Linux GNU toolchain target");
  requireValue(posix.basename(policy.dylint_drivers) === ".dylint_drivers",
    "dylint_drivers must use the protected HOME/.dylint_drivers location");
  const paths = RUST_PATHS.map((key) => policy[key]);
  requireValue(paths.every((path, index) => paths.every((other, otherIndex) =>
    index === otherIndex || (path !== other && !path.startsWith(`${other}/`)))),
  "Rust tool and dependency roots must be separate");
  requireValue(paths.every((path) => path !== policy.cargo_target
    && !policy.cargo_target.startsWith(`${path}/`) && !path.startsWith(`${policy.cargo_target}/`)),
  "mutable Cargo output must be separate from immutable Rust tooling");
}

export function rustEnvironment(policy, environment = process.env) {
  validateRustPolicy(policy);
  const expected = {
    PATH: `${policy.rust_bin}:/opt/bureau/bin:/usr/local/bin:/usr/bin:/bin`,
    HOME: posix.dirname(policy.dylint_drivers),
    CARGO_HOME: policy.cargo_home, RUSTUP_HOME: policy.rustup_home,
    CARGO_NET_OFFLINE: "true", RUSTUP_AUTO_INSTALL: "0",
  };
  for (const [key, value] of Object.entries(expected)) {
    requireValue(environment[key] === value, `maintenance runtime requires ${key}=${value}`);
  }
  for (const key of Object.keys(environment)) {
    requireValue(key in expected || !/^(?:CARGO_|RUST|DYLINT_|LD_|DYLD_|NODE_OPTIONS$|BASH_ENV$|ENV$)/u.test(key),
      `unapproved maintenance runtime override: ${key}`);
  }
  return childEnvironment({ CARGO_HOME: policy.cargo_home, RUSTUP_HOME: policy.rustup_home }, environment);
}

function protectedOwner(info, inspect) {
  // The engine maps the daemon to uid 0; host-root files then have an unmapped uid.
  return Number.isSafeInteger(info.uid) && info.uid >= 0
    && (inspect.rootOwner ? info.uid === 0 : info.uid !== inspect.uid());
}

async function protectedDirectory(path, inspect) {
  requireValue(await inspect.realpath(path) === path, `Rust directory must be canonical: ${path}`);
  let ancestor = path;
  for (;;) {
    const info = await inspect.lstat(ancestor);
    requireValue(info.isDirectory() && !info.isSymbolicLink() && protectedOwner(info, inspect) && (info.mode & 0o022) === 0,
      `Rust executable/cache ancestry must be operator-owned and protected: ${ancestor}`);
    if (ancestor === "/") break;
    ancestor = posix.dirname(ancestor);
  }
  requireValue((await inspect.filesystemSnapshot(path)).readOnly,
    `Rust tooling must be on an explicitly read-only mount: ${path}`);
}

async function protectedFile(path, inspect, executable = true) {
  requireValue(await inspect.realpath(path) === path, `Rust file must not be a symlink: ${path}`);
  const info = await inspect.lstat(path);
  requireValue(info.isFile() && !info.isSymbolicLink() && protectedOwner(info, inspect) && (info.mode & 0o222) === 0
    && (!executable || (info.mode & 0o111) !== 0), `Rust file must be provisioned immutable tooling: ${path}`);
  await protectedDirectory(posix.dirname(path), inspect);
}

async function requireProxies(policy, inspect) {
  const rustup = posix.join(policy.rust_bin, "rustup");
  for (const command of ["rustup", "cargo-dylint", "dylint-link"]) {
    await protectedFile(posix.join(policy.rust_bin, command), inspect);
  }
  for (const command of PROXIES) {
    const path = posix.join(policy.rust_bin, command);
    const info = await inspect.lstat(path);
    requireValue(info.isSymbolicLink() && protectedOwner(info, inspect) && await inspect.realpath(path) === rustup,
      `maintenance requires the genuine rustup proxy: ${path}`);
  }
}

async function toolchain(root, path, pattern, inspect) {
  const text = (await inspect.read(join(root, path), 8192)).toString("utf8");
  const channels = [...text.matchAll(/^\s*channel\s*=\s*"([^"]+)"\s*$/gmu)];
  requireValue(channels.length === 1 && pattern.test(channels[0][1]), `invalid reviewed toolchain pin: ${path}`);
  return channels[0][1];
}

async function toolchains(root, policy, inspect) {
  const stable = await toolchain(root, "rust-toolchain.toml", /^\d+\.\d+\.\d+$/u, inspect);
  const nightly = await toolchain(root, "lints/rust-lints/rust-toolchain", /^nightly-\d{4}-\d{2}-\d{2}$/u, inspect);
  const sysroot = (channel) => posix.join(policy.rustup_home, "toolchains", `${channel}-${policy.rust_host}`);
  for (const channel of [stable, nightly]) {
    for (const command of ["cargo", "rustc", "rustdoc"]) {
      await protectedFile(posix.join(sysroot(channel), "bin", command), inspect);
    }
    await protectedDirectory(posix.join(sysroot(channel), "lib"), inspect);
  }
  for (const command of ["rustfmt", "cargo-fmt", "clippy-driver", "cargo-clippy"]) {
    await protectedFile(posix.join(sysroot(stable), "bin", command), inspect);
  }
  const libraryPath = posix.join(sysroot(nightly), "lib", "rustlib", policy.rust_host, "lib");
  await protectedDirectory(libraryPath, inspect);
  const libraries = await inspect.readdir(libraryPath);
  const drivers = libraries.filter((name) => /^librustc_driver-[a-f0-9]+\.so$/u.test(name));
  requireValue(drivers.length === 1, "the pinned nightly rustc-dev driver must be provisioned");
  await protectedFile(posix.join(libraryPath, drivers[0]), inspect, false);
  return { stable, nightly, sysroot };
}

function probe(command, args, options) {
  try {
    return execFileSync(command, args, { ...options, encoding: "utf8", timeout: 10_000, maxBuffer: 64 * 1024 }).trim();
  } catch (error) {
    throw new Error(`prepared Rust command failed (${posix.basename(command)} ${args.join(" ")}): ${error.message}`);
  }
}

export async function requirePreparedRust(root, policy, {
  environment = process.env, inspect = INSPECT, execute = probe, rootOwner = false,
} = {}) {
  inspect = { ...inspect, rootOwner };
  const env = rustEnvironment(policy, environment);
  for (const key of RUST_PATHS) await protectedDirectory(policy[key], inspect);
  await requireProxies(policy, inspect);
  const { stable, nightly, sysroot } = await toolchains(root, policy, inspect);
  const cargoFiles = await inspect.readdir(policy.cargo_home);
  requireValue(!cargoFiles.some((name) => ["credentials", "credentials.toml", "config", "config.toml"].includes(name)),
    "prepared Cargo home must not contain credentials or configuration overrides");
  for (const directory of ["registry", "git"]) {
    await protectedDirectory(posix.join(policy.cargo_home, directory), inspect);
  }
  const driver = posix.join(policy.dylint_drivers, `${nightly}-${policy.rust_host}`, "dylint-driver");
  await protectedFile(driver, inspect);
  const run = (command, args, cwd = root) => execute(posix.join(policy.rust_bin, command), args, { cwd, env });
  for (const [channel, prefix, cwd] of [[stable, [], root], [nightly, [`+${nightly}`], root],
    [nightly, [], join(root, "lints", "rust-lints")]]) {
    requireValue(run("rustc", [...prefix, "--print", "sysroot"], cwd) === sysroot(channel),
      `rustup did not select the reviewed ${channel} toolchain`);
  }
  requireValue(run("cargo", ["--version"]).startsWith(`cargo ${stable} `), "stable Cargo version differs from the reviewed pin");
  const dylint = run("cargo", ["dylint", "--version"]);
  const version = run("rustup", ["run", nightly, driver, "-V"]);
  requireValue(dylint.split(/\s+/u).at(-1) === DYLINT_VERSION && version.split(/\s+/u).at(-1) === DYLINT_VERSION,
    `cargo-dylint and the immutable nightly driver must both be ${DYLINT_VERSION}`);
  return { CARGO_HOME: policy.cargo_home, RUSTUP_HOME: policy.rustup_home };
}
