//! Shared local lease inspection without creating or migrating state.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, params};

use super::InspectionError;

/// Active and expired lease row counts at one observation time.
#[derive(Default)]
pub struct LeaseCounts {
    pub active: usize,
    pub expired: usize,
}

fn open_read_only(path: &Path) -> Result<Connection, InspectionError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| InspectionError::StateDb(format!("{}: {error}", path.display())))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(InspectionError::StateDb(format!(
            "{} is not a safe state database",
            path.display()
        )));
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| InspectionError::StateDb(error.to_string()))
}

fn query_count(
    connection: &Connection,
    predicate_or_sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<usize, InspectionError> {
    let sql = if predicate_or_sql.starts_with("SELECT") {
        predicate_or_sql.to_owned()
    } else {
        format!("SELECT COUNT(*) FROM leases WHERE {predicate_or_sql}")
    };
    let count: i64 = connection
        .query_row(&sql, parameters, |row| row.get(0))
        .map_err(|error| InspectionError::StateDb(error.to_string()))?;
    usize::try_from(count).map_err(|error| InspectionError::StateDb(error.to_string()))
}

/// Milliseconds since the Unix epoch. The process clock boundary:
/// bound once as a function pointer so this stays the single read site.
pub fn now_millis() -> u64 {
    let now = SystemTime::now;
    now().duration_since(UNIX_EPOCH).map_or(0, |duration| {
        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    })
}

pub fn lease_counts(path: &Path) -> Result<LeaseCounts, InspectionError> {
    if !path.exists() {
        return Ok(LeaseCounts::default());
    }
    let connection = open_read_only(path)?;
    let now = i64::try_from(now_millis()).unwrap_or(i64::MAX);
    let active = query_count(&connection, "expires_at_ms > ?1", (now,))?;
    let expired = query_count(&connection, "expires_at_ms <= ?1", (now,))?;
    Ok(LeaseCounts { active, expired })
}

/// Counts unexpired leases, optionally for one durable run.
///
/// # Errors
/// Rejects unsafe state paths and propagates read-only `SQLite` failures.
pub fn active_lease_count(path: &Path, run_id: Option<&str>) -> Result<usize, InspectionError> {
    if !path.exists() {
        return Ok(0);
    }
    let connection = open_read_only(path)?;
    let now = i64::try_from(now_millis()).unwrap_or(i64::MAX);
    run_id.map_or_else(
        || query_count(&connection, "expires_at_ms > ?1", (now,)),
        |run_id| {
            let sql = "SELECT COUNT(*) FROM leases WHERE run_id = ?1 AND expires_at_ms > ?2";
            query_count(&connection, sql, params![run_id, now])
        },
    )
}
