//! Immutable admission evidence for rate counters, not pending work or queue state.

use std::io;

use rusqlite::{Connection, TransactionBehavior};

use super::{Error, sql};

#[cfg(test)]
mod tests;

const VERSION: i64 = 1;
const COLUMNS: &str = "
SELECT a.assignment, a.forge, a.external_id, a.run_id, a.admitted_at_ms, a.keep_legacy_run,
       r.run_id, r.assignment, r.started_at_ms, r.cost_usd,
       l.assignment, l.forge, l.external_id, l.run_id, l.owner_id, l.expires_at_ms
FROM run_admissions AS a, runs AS r, leases AS l LIMIT 0";
const EXISTS: &str =
    "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'run_admissions')";

const SCHEMA: &str = "
CREATE TABLE run_admissions (
    assignment TEXT NOT NULL,
    forge TEXT NOT NULL,
    external_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    admitted_at_ms INTEGER NOT NULL,
    keep_legacy_run INTEGER NOT NULL DEFAULT 0 CHECK (keep_legacy_run IN (0, 1)),
    PRIMARY KEY (assignment, forge, external_id, run_id)
);
CREATE INDEX admissions_by_assignment_time
ON run_admissions (assignment, admitted_at_ms);
CREATE INDEX admissions_by_run ON run_admissions (assignment, run_id);
CREATE INDEX IF NOT EXISTS runs_by_assignment_time ON runs (assignment, started_at_ms);
";

const BACKFILL: &str = "
INSERT INTO run_admissions (
    assignment, forge, external_id, run_id, admitted_at_ms, keep_legacy_run
)
SELECT l.assignment, l.forge, l.external_id, l.run_id,
    CASE WHEN l.run_id GLOB 'legacy-[0-9]*' AND r.run_id IS NOT NULL
        THEN ?1 ELSE COALESCE(r.started_at_ms, ?1) END,
    l.run_id GLOB 'legacy-[0-9]*' AND r.run_id IS NOT NULL
FROM leases AS l
LEFT JOIN runs AS r ON r.assignment = l.assignment AND r.run_id = l.run_id";

const RECORD: &str = "
INSERT INTO run_admissions (assignment, forge, external_id, run_id, admitted_at_ms)
VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT (assignment, forge, external_id, run_id) DO NOTHING";
const RECORDED: &str = "
SELECT admitted_at_ms FROM run_admissions
WHERE assignment = ?1 AND forge = ?2 AND external_id = ?3 AND run_id = ?4";

const RUNS_SINCE: &str = "
SELECT COUNT(*) FROM (
    SELECT 1 FROM run_admissions
    WHERE assignment = ?1 AND admitted_at_ms > ?2
    UNION ALL
    SELECT 1 FROM runs
    WHERE assignment = ?1 AND started_at_ms > ?2
      AND NOT EXISTS (
        SELECT 1 FROM run_admissions AS a
        WHERE a.assignment = runs.assignment AND a.run_id = runs.run_id
          AND a.keep_legacy_run = 0
      )
)";

const LEGACY_RUNS_SINCE: &str = "
SELECT COUNT(*) FROM (
    SELECT 1 FROM runs WHERE assignment = ?1 AND started_at_ms > ?2
    UNION ALL
    SELECT 1 FROM leases WHERE assignment = ?1
)";

fn invalid(message: impl Into<String>) -> Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

fn legacy_schema(conn: &Connection) -> Result<bool, Error> {
    let exists: bool = conn.query_row(EXISTS, [], |row| row.get(0))?;
    if exists {
        return Err(invalid(
            "unversioned admission accounting cannot be migrated safely",
        ));
    }
    Ok(false)
}

/// Checks the version before any schema creation can hide missing accounting evidence.
pub(super) fn schema_ready(conn: &Connection) -> Result<bool, Error> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => legacy_schema(conn),
        VERSION => {
            drop(conn.prepare(COLUMNS)?);
            Ok(true)
        }
        _ => Err(invalid(format!(
            "unsupported state accounting version {version}"
        ))),
    }
}

/// Retains undated legacy leases, including expired ones, at their first observation.
/// Synthetic legacy identities may collide across old tables; never merge those histories.
pub(super) fn migrate(conn: &mut Connection, now: i64) -> Result<(), Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if schema_ready(&tx)? {
        return Ok(());
    }
    tx.execute_batch(SCHEMA)?;
    tx.execute(BACKFILL, [now])?;
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
    Ok(())
}

/// Records a winning claim inside its transaction; renewal and recovery cannot retime it.
pub(super) fn record(
    conn: &Connection,
    assignment: &str,
    forge: &str,
    external_id: &str,
    run_id: &str,
    now: i64,
) -> Result<(), Error> {
    conn.execute(RECORD, (assignment, forge, external_id, run_id, now))?;
    conn.query_row(RECORDED, (assignment, forge, external_id, run_id), |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(())
}

/// Read-only legacy views conservatively count every retained, undated lease.
pub(super) fn runs_since(conn: &Connection, assignment: &str, since: i64) -> Result<u32, Error> {
    let statement = if schema_ready(conn)? {
        RUNS_SINCE
    } else {
        LEGACY_RUNS_SINCE
    };
    sql::count(conn, statement, assignment, since)
}
