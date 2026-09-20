import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { NATIVE_BUDGET } from "./budget.mjs";

export const REPORTER_ENV = "/var/lib/bureau-maintenance/credentials/github-reporter.env";
export const OWNER_PATH = "/opt/bureau/rust/bin:/opt/bureau/bin:/usr/local/bin:/usr/bin:/bin";
export const PROFILE_PROPERTIES = ["FragmentPath", "DropInPaths", "EnvironmentFiles", "NeedDaemonReload",
  "RefuseManualStart", "Restart", "BindsTo", "After", "TimeoutStopUSec", "KillMode", "SendSIGKILL", "User", "MemoryMax"];

export function serviceProfile(state, engine, guard) {
  const unit = `/etc/systemd/system/${engine}`;
  const drops = ["20-reporter-credential.conf", "windows-supervision.conf"].map((name) => `${unit}.d/${name}`);
  requireValue(state.FragmentPath === unit && typeof state.DropInPaths === "string"
    && state.DropInPaths.split(" ").sort().join(" ") === drops.sort().join(" ")
    && state.EnvironmentFiles === `/etc/bureau/maintenance.env (ignore_errors=no) ${REPORTER_ENV} (ignore_errors=no)`
    && state.NeedDaemonReload === "no" && state.RefuseManualStart === "yes"
    && state.Restart === "no" && state.BindsTo === guard && typeof state.After === "string"
    && state.After.split(" ").includes(guard) && state.TimeoutStopUSec === "5s"
    && state.KillMode === "mixed" && state.SendSIGKILL === "yes" && state.User === "bureau"
    && state.MemoryMax === String(NATIVE_BUDGET.engineMemoryMaxBytes),
  "supervised service or required reporter binding is not effective");
}

export function reporterMetadata(info, uid, gid) {
  requireValue(Number.isSafeInteger(uid) && uid > 0 && Number.isSafeInteger(gid) && gid > 0,
    "dedicated service account identity is missing");
  requireValue(Number.isSafeInteger(info.uid) && Number.isSafeInteger(info.gid)
    && Number.isSafeInteger(info.mode) && info.mode >= 0, "reporter file ownership is unobservable");
  requireValue(info.isFile() && !info.isSymbolicLink() && info.uid === 0 && info.gid === 0
    && (info.mode & 0o777) === 0o600
    && Number.isSafeInteger(info.size) && info.size > 0 && info.size <= 8192,
  "required reporter environment must be a private regular file");
}
