//! `SQL` statements and row mapping for the state store, kept apart from
//! `state.rs` so the public surface stays readable. Times are integer
//! milliseconds since the Unix epoch; `SQLite` has no unsigned 64-bit
//! integer, so values cross the boundary as `i64`.

use rusqlite::{Connection, ErrorCode, Row};

use super::{Error, Lease};

/// The schema, applied at open. `IF NOT EXISTS` keeps open idempotent.
pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS leases (
    assignment TEXT NOT NULL,
    forge TEXT NOT NULL,
    external_id TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    UNIQUE (assignment, forge, external_id)
);
CREATE TABLE IF NOT EXISTS runs (
    assignment TEXT NOT NULL,
    started_at_ms INTEGER NOT NULL,
    cost_usd REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS dedup (
    content_hash TEXT PRIMARY KEY,
    disposition TEXT NOT NULL,
    at_ms INTEGER NOT NULL
);
";

/// Drops the key's lease only when it has already expired.
pub(super) const REAP_EXPIRED: &str = "
DELETE FROM leases
WHERE assignment = ?1 AND forge = ?2 AND external_id = ?3
    AND expires_at_ms <= ?4";

/// The claim itself; conflicts on the unique index mean "already held".
pub(super) const INSERT_LEASE: &str = "
INSERT INTO leases (assignment, forge, external_id, expires_at_ms)
VALUES (?1, ?2, ?3, ?4)";

pub(super) const RELEASE: &str = "DELETE FROM leases WHERE assignment = ?1 AND external_id = ?2";

/// Extends only a lease that exists and has not expired.
pub(super) const RENEW: &str = "
UPDATE leases SET expires_at_ms = ?1
WHERE assignment = ?2 AND external_id = ?3 AND expires_at_ms > ?4";

pub(super) const ACTIVE_LEASES: &str = "
SELECT assignment, forge, external_id, expires_at_ms FROM leases
WHERE assignment = ?1 AND expires_at_ms > ?2";

pub(super) const LIVE_LEASES: &str = "
SELECT COUNT(*) FROM leases
WHERE assignment = ?1 AND expires_at_ms > ?2";

pub(super) const RUNS_SINCE: &str = "
SELECT COUNT(*) FROM runs
WHERE assignment = ?1 AND started_at_ms > ?2";

pub(super) const COST_SINCE: &str = "
SELECT COALESCE(SUM(cost_usd), 0.0) FROM runs
WHERE assignment = ?1 AND started_at_ms > ?2";

pub(super) const RECORD_RUN: &str = "
INSERT INTO runs (assignment, started_at_ms, cost_usd)
VALUES (?1, ?2, ?3)";

pub(super) const SEEN: &str = "SELECT EXISTS(SELECT 1 FROM dedup WHERE content_hash = ?1)";

pub(super) const MARK_SEEN: &str = "
INSERT OR REPLACE INTO dedup (content_hash, disposition, at_ms)
VALUES (?1, ?2, ?3)";

/// Maps one `leases` row to a [`Lease`].
pub(super) fn lease_from_row(row: &Row<'_>) -> rusqlite::Result<Lease> {
    let expires: i64 = row.get(3)?;
    Ok(Lease {
        assignment: row.get(0)?,
        forge: row.get(1)?,
        external_id: row.get(2)?,
        expires_at_ms: u64::try_from(expires).unwrap_or(0),
    })
}

/// Runs a `COUNT(*)` query scoped to an assignment and a time bound.
pub(super) fn count(
    conn: &Connection,
    statement: &str,
    assignment: &str,
    since_ms: i64,
) -> Result<u32, Error> {
    let params = (assignment, since_ms);
    let count: i64 = conn.query_row(statement, params, |row| row.get(0))?;
    Ok(u32::try_from(count).unwrap_or(0))
}

/// Total recorded run cost for an assignment since `since_ms`.
pub(super) fn cost_since(conn: &Connection, assignment: &str, since_ms: i64) -> Result<f64, Error> {
    let spent = conn.query_row(COST_SINCE, (assignment, since_ms), |row| row.get(0))?;
    Ok(spent)
}

/// Whether the error is the unique-index conflict that means "claimed".
pub(super) fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == ErrorCode::ConstraintViolation
    )
}
