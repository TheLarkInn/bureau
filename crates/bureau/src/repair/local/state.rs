use std::fs;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::doctor::replay_run_read_only;
use crate::doctor::state_db::{active_lease_count, now_millis};
use crate::home::Layout;
use crate::repair::Ownership;
use crate::runlog;

use super::files::safe_run_directory;

pub(super) fn has_any_live_run(layout: &Layout) -> Result<bool, String> {
    active_lease_count(layout.state_db(), None).map(|count| count > 0)
}

pub(super) fn has_live_run(layout: &Layout, run_id: &str) -> Result<bool, String> {
    active_lease_count(layout.state_db(), Some(run_id)).map(|count| count > 0)
}

pub(super) fn reap_expired(layout: &Layout, ownership: &Ownership) -> Result<(), String> {
    let connection = open_state(layout.state_db(), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let expires = i64::try_from(ownership.expires_at_ms)
        .map_err(|_| "ownership expiry is outside SQLite range".to_owned())?;
    let now = i64::try_from(now_millis()).unwrap_or(i64::MAX);
    let changed = connection
        .execute(REAP_EXACT, ownership_params(ownership, expires, now))
        .map_err(|error| error.to_string())?;
    if changed == 1 {
        Ok(())
    } else {
        Err("ownership changed, is live, or is already absent".to_owned())
    }
}

pub(super) fn rebuild_derived(layout: &Layout, run_id: &str) -> Result<(), String> {
    if has_live_run(layout, run_id)? {
        return Err(format!("run `{run_id}` still has live evidence"));
    }
    let directory = safe_run_directory(layout, run_id)?;
    let state = replay_run_read_only(&directory)?;
    runlog::write_state_cache(&directory, &state).map_err(|error| error.to_string())
}

const REAP_EXACT: &str = "
DELETE FROM leases
WHERE assignment = ?1 AND forge = ?2 AND external_id = ?3
  AND run_id = ?4 AND owner_id = ?5 AND expires_at_ms = ?6
  AND expires_at_ms <= ?7";

fn ownership_params(
    ownership: &Ownership,
    expires: i64,
    now: i64,
) -> (&str, &str, &str, &str, &str, i64, i64) {
    (
        &ownership.assignment,
        &ownership.forge,
        &ownership.external_id,
        &ownership.run_id,
        &ownership.owner_id,
        expires,
        now,
    )
}

fn open_state(path: &Path, flags: OpenFlags) -> Result<Connection, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{} is not a safe state database", path.display()));
    }
    Connection::open_with_flags(path, flags).map_err(|error| error.to_string())
}
