import { execFile } from "node:child_process";
import { lstat, readFile, realpath } from "node:fs/promises";
import { posix } from "node:path";
import { promisify } from "node:util";

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { readBoundedFile } from "../../scripts/maintenance-files.mjs";
import { filesystemSnapshot } from "../../scripts/maintenance-mount.mjs";
import { COMMIT, ID, LIMITS, identity } from "./protocol.mjs";
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

export async function properties(unit, names) {
  requireValue(typeof unit === "string" && /^[a-zA-Z0-9_.@-]+\.service$/u.test(unit), "invalid exact service name");
  const text = await command("/usr/bin/systemctl", ["show", unit, "--no-pager", `--property=${names.join(",")}`]);
  const entries = text.split("\n").map((line) => {
    const offset = line.indexOf("=");
    requireValue(offset > 0, "malformed systemd observation");
    return [line.slice(0, offset), line.slice(offset + 1)];
  });
  const result = Object.fromEntries(entries);
  requireValue(entries.length === names.length && Object.keys(result).length === names.length
    && names.every((name) => name in result), "incomplete systemd observation");
  return result;
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

export async function processIdentity(state, inspect = { readFile, realpath }) {
  const pid = Number(state.MainPID);
  requireValue(typeof state.InvocationID === "string" && ID.test(state.InvocationID)
    && typeof state.MainPID === "string" && /^[1-9]\d*$/u.test(state.MainPID)
    && typeof state.ControlGroup === "string" && Number.isSafeInteger(pid) && pid > 1,
  "running service has no owned identity");
  const stat = await inspect.readFile(`/proc/${pid}/stat`, "utf8");
  const end = stat.lastIndexOf(")");
  const fields = stat.slice(end + 2).trim().split(/\s+/u);
  requireValue(stat.startsWith(`${pid} (`) && end > 0 && fields.length >= 22
    && !["Z", "X"].includes(fields[0]), "owned process is absent or dead");
  const group = await inspect.readFile(`/proc/${pid}/cgroup`, "utf8");
  requireValue(group.trim() === `0::${state.ControlGroup}`, "owned process escaped its service cgroup");
  const result = identity({ invocation: state.InvocationID, pid, starttime: fields[19], cgroup: state.ControlGroup });
  return { identity: result, executable: await inspect.realpath(`/proc/${pid}/exe`) };
}

export async function serviceState(unit) {
  return properties(unit, ["ActiveState", "SubState", "InvocationID", "MainPID", "ControlPID", "ControlGroup"]);
}

export async function emptyService(unit) {
  const state = await serviceState(unit);
  requireValue(["inactive", "failed"].includes(state.ActiveState) && state.MainPID === "0", "service has not drained");
  if (state.ControlGroup) {
    const text = await readFile(`/sys/fs/cgroup${state.ControlGroup}/cgroup.events`, "utf8");
    requireValue(/^populated 0$/mu.test(text), "service cgroup is still populated");
  }
  return state;
}
