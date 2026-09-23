import { execFile } from "node:child_process";
import { lstat, readFile, realpath } from "node:fs/promises";
import { posix } from "node:path";
import { promisify } from "node:util";

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { readBoundedFile } from "../../scripts/maintenance-files.mjs";
import { filesystemSnapshot } from "../../scripts/maintenance-mount.mjs";
import { COMMIT, ID, LIMITS, identity, sameIdentity } from "./protocol.mjs";
import { binaryProvenance } from "./provenance.mjs";
import { PROFILE_PROPERTIES, REPORTER_ENV, reporterMetadata, serviceProfile } from "./profile.mjs";

export const ROOT = "/opt/bureau/source";
export const ENGINE = "bureau-maintenance.service";
export const GUARD = "bureau-windows-heartbeat.service";
export const RUNTIME = "/run/bureau-windows-supervision";
export const ENV = Object.freeze({
  PATH: "/usr/bin:/bin", LANG: "C", LC_ALL: "C", HOME: "/nonexistent",
  GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null", GIT_OPTIONAL_LOCKS: "0",
});
const execute = promisify(execFile);
const REPEATED_PROPERTIES = new Set(["EnvironmentFiles"]);

export async function command(file, args, env = ENV) {
  try {
    const result = await execute(file, args, { env, encoding: "utf8", timeout: LIMITS.command,
      killSignal: "SIGKILL", maxBuffer: 1024 * 1024 });
    requireValue(result.stderr === "", "unexpected supervision command diagnostics");
    return result.stdout.trim();
  } catch {
    throw new Error(`supervision command failed: ${posix.basename(file)}`);
  }
}

export function parseProperties(text, names) {
  const expected = new Set(names);
  requireValue(expected.size === names.length, "incomplete systemd observation");
  const values = new Map();
  for (const line of text.split("\n")) {
    const offset = line.indexOf("=");
    requireValue(offset > 0, "malformed systemd observation");
    const name = line.slice(0, offset);
    requireValue(expected.has(name), "incomplete systemd observation");
    requireValue(!values.has(name) || REPEATED_PROPERTIES.has(name), "incomplete systemd observation");
    const current = values.get(name) ?? [];
    values.set(name, [...current, line.slice(offset + 1)]);
  }
  requireValue(values.size === names.length, "incomplete systemd observation");
  return Object.fromEntries(names.map((name) => [name, values.get(name).join(" ")]));
}

export async function properties(unit, names) {
  requireValue(typeof unit === "string" && /^[a-zA-Z0-9_.@-]+\.service$/u.test(unit), "invalid exact service name");
  const text = await command("/usr/bin/systemctl", ["show", unit, "--no-pager", `--property=${names.join(",")}`]);
  return parseProperties(text, names);
}

export async function protectedPath(path, directory = false) {
  requireValue(await realpath(path) === path, "supervision path must be canonical");
  let current = path;
  for (;;) {
    const info = await lstat(current);
    requireValue(info.uid === 0 && (info.mode & 0o022) === 0 && !info.isSymbolicLink()
      && (current === path && !directory ? info.isFile() : info.isDirectory()),
    "supervision path or ancestor is not root-protected");
    if (current === "/") break;
    current = posix.dirname(current);
  }
}

export function approval(text, commit) {
  requireValue(typeof text === "string" && typeof commit === "string" && COMMIT.test(commit),
    "deployment approval requires textual reviewed provenance");
  const rows = text.split("\n").map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
  requireValue(rows.length === 2 && new Set(rows).size === 2
    && rows.includes(`BUREAU_CONFIG_COMMIT=${commit}`)
    && rows.includes("BUREAU_DEPLOYMENT_APPROVED=true"), "deployment approval or reviewed commit differs");
}

export async function installation(commit) {
  requireValue(process.platform === "linux" && process.getuid() === 0 && typeof commit === "string" && COMMIT.test(commit),
    "supervision requires root on the explicitly provisioned Linux host");
  await protectedPath(ROOT, true);
  requireValue((await filesystemSnapshot(ROOT)).readOnly, "reviewed supervision source must be read-only");
  const git = (args) => command("/usr/bin/git", ["-c", `safe.directory=${ROOT}`, "-c", "core.fsmonitor=false",
    "-c", "core.hooksPath=/dev/null", "-C", ROOT, ...args]);
  requireValue(await git(["rev-parse", "HEAD"]) === commit, "installed source differs from the reviewed commit");
  const tracked = await git(["ls-files", "-v"]);
  requireValue(tracked.length > 0 && tracked.split("\n").every((line) => line.startsWith("H ")),
    "installed source has index-hiding or unmerged paths");
  requireValue(await git(["status", "--porcelain=v1", "--untracked-files=normal"]) === "", "installed source is not clean");
  const env = "/etc/bureau/maintenance.env";
  await protectedPath(env);
  approval((await readBoundedFile(env, 8192)).toString("utf8"), commit);
  await binaryProvenance(commit, protectedPath);
  await protectedPath(await realpath("/usr/bin/python3"));
  await command("/usr/bin/python3", ["-I", "-S", "-c",
    "import socket,sys; assert sys.version_info >= (3,8); assert hasattr(socket, 'SO_PEERCRED')"]);
  const unit = `/etc/systemd/system/${ENGINE}`;
  const files = [[unit, "deployment/bureau-maintenance.service"],
    ...["20-reporter-credential.conf", "windows-supervision.conf"].map((name) => [`${unit}.d/${name}`, `deployment/${name}`])];
  for (const [installed, source] of files) {
    await protectedPath(installed);
    requireValue((await readBoundedFile(installed, 16_384)).equals(await readBoundedFile(`${ROOT}/${source}`, 16_384)),
      "installed service/profile differs from reviewed source");
  }
  serviceProfile(await properties(ENGINE, PROFILE_PROPERTIES), ENGINE, GUARD);
  const uid = Number(await command("/usr/bin/id", ["-u", "bureau"]));
  const gid = Number(await command("/usr/bin/id", ["-g", "bureau"]));
  requireValue(await realpath(REPORTER_ENV) === REPORTER_ENV, "required reporter path must be canonical");
  reporterMetadata(await lstat(REPORTER_ENV), uid, gid);
}

function serviceProcess(state) {
  const pid = Number(state.MainPID);
  requireValue(typeof state.InvocationID === "string" && ID.test(state.InvocationID)
    && typeof state.MainPID === "string" && /^[1-9]\d*$/u.test(state.MainPID)
    && typeof state.ControlGroup === "string" && Number.isSafeInteger(pid) && pid > 1,
  "running service has no owned identity");
  return pid;
}

async function processRecord(state, inspect) {
  const pid = serviceProcess(state);
  const stat = await inspect.readFile(`/proc/${pid}/stat`, "utf8");
  const end = stat.lastIndexOf(")");
  const fields = stat.slice(end + 2).trim().split(/\s+/u);
  requireValue(stat.startsWith(`${pid} (`) && end > 0 && stat[end + 1] === " " && fields.length >= 22
    && /^[RSDZTtXxKWPI]$/u.test(fields[0]), "malformed owned process stat");
  const result = identity({ invocation: state.InvocationID, pid, starttime: fields[19], cgroup: state.ControlGroup });
  return { identity: result, status: ["Z", "X", "x"].includes(fields[0]) ? "exited" : "running" };
}

async function processCgroup(owned, inspect) {
  const group = await inspect.readFile(`/proc/${owned.pid}/cgroup`, "utf8");
  return group.trim();
}

async function processExecutable(owned, inspect) {
  requireValue(await processCgroup(owned, inspect) === `0::${owned.cgroup}`, "owned process escaped its service cgroup");
  return inspect.realpath(`/proc/${owned.pid}/exe`);
}

export async function processIdentity(state, inspect = { readFile, realpath }) {
  const observed = await processRecord(state, inspect);
  requireValue(observed.status === "running", "owned process is absent or dead");
  return { identity: observed.identity, executable: await processExecutable(observed.identity, inspect) };
}

export async function drainProcessIdentity(state, owned, inspect = { readFile, realpath }) {
  const pid = serviceProcess(state);
  requireValue(state.InvocationID === owned.invocation && pid === owned.pid && state.ControlGroup === owned.cgroup,
    "engine process changed during drain");
  try {
    const observed = await processRecord(state, inspect);
    requireValue(sameIdentity(observed.identity, owned), "engine process changed during drain");
    // Exited leaders can lose their executable and cgroup membership before reaping.
    if (observed.status === "exited") return observed;
    const group = await processCgroup(owned, inspect);
    if (group !== `0::${owned.cgroup}`) {
      requireValue(group === "0::/", "owned process escaped its service cgroup");
      const repeated = await processRecord(state, inspect);
      requireValue(sameIdentity(repeated.identity, owned), "engine process changed during drain");
      requireValue(repeated.status === "exited", "owned process escaped its service cgroup");
      return repeated;
    }
    await inspect.realpath(`/proc/${pid}/exe`);
    return observed;
  } catch (error) {
    if (!["ENOENT", "ESRCH"].includes(error.code)) throw error;
    return { status: "absent", identity: null };
  }
}

export async function serviceState(unit) {
  return properties(unit, ["ActiveState", "SubState", "InvocationID", "MainPID", "ControlPID", "ControlGroup"]);
}

function terminalService(state, owned) {
  requireValue(["inactive", "failed"].includes(state.ActiveState) && state.MainPID === "0", "service has not drained");
  requireValue(!owned || ((!state.InvocationID || state.InvocationID === owned.invocation)
    && (!state.ControlGroup || state.ControlGroup === owned.cgroup)), "service ownership changed during drain");
}

async function removedCgroup(path, inspect) {
  try {
    await inspect.lstat(path);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    const parent = await inspect.lstat(posix.dirname(path));
    requireValue(parent.isDirectory() && !parent.isSymbolicLink(), "cgroup parent is unobservable");
    return;
  }
  throw new Error("existing cgroup has no observable population counter");
}

async function emptyCgroup(group, inspect) {
  requireValue(typeof group === "string" && /^\/system\.slice\/[a-zA-Z0-9_.@-]+\.service$/u.test(group),
    "service cgroup is unobservable");
  const path = `/sys/fs/cgroup${group}`;
  let text;
  try {
    text = await inspect.readFile(`${path}/cgroup.events`, "utf8");
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    await removedCgroup(path, inspect);
    return;
  }
  const population = text.split("\n").filter((line) => line.startsWith("populated"));
  requireValue(population.length === 1 && population[0] === "populated 0", "service cgroup is still populated or unobservable");
}

export async function emptyService(unit, owned, inspect = { show: serviceState, readFile, lstat }) {
  const state = await inspect.show(unit);
  terminalService(state, owned);
  const group = owned?.cgroup || state.ControlGroup;
  if (group) await emptyCgroup(group, inspect);
  const final = await inspect.show(unit);
  terminalService(final, owned);
  return final;
}
