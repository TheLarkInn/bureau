//! `SQL` statements and row mapping for the state store, kept apart from
//! `state.rs` so the public surface stays readable. Times are integer
//! milliseconds since the Unix epoch; `SQLite` has no unsigned 64-bit
//! integer, so values cross the boundary as `i64`.

use rusqlite::{Connection, ErrorCode, OptionalExtension, Row};

use super::{Error, Lease};

/// The schema, applied at open. `IF NOT EXISTS` keeps open idempotent.
pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS leases (
    assignment TEXT NOT NULL,
    forge TEXT NOT NULL,
    external_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    UNIQUE (assignment, forge, external_id)
);
CREATE TABLE IF NOT EXISTS runs (
    run_id TEXT PRIMARY KEY,
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
INSERT INTO leases (assignment, forge, external_id, run_id, owner_id, expires_at_ms)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

pub(super) const RELEASE: &str = "
DELETE FROM leases
WHERE assignment = ?1 AND external_id = ?2 AND run_id = ?3 AND owner_id = ?4";

pub(super) const RELEASE_RUN: &str =
    "DELETE FROM leases WHERE assignment = ?1 AND external_id = ?2 AND run_id = ?3";

/// Extends only a lease that exists and has not expired.
pub(super) const RENEW: &str = "
UPDATE leases SET expires_at_ms = ?1
WHERE assignment = ?2 AND external_id = ?3 AND run_id = ?4 AND owner_id = ?5
    AND expires_at_ms > ?6";

pub(super) const OWNED: &str = "
SELECT EXISTS(
    SELECT 1 FROM leases
    WHERE assignment = ?1 AND external_id = ?2 AND run_id = ?3 AND owner_id = ?4
        AND expires_at_ms > ?5
)";

pub(super) const ACTIVE_LEASES: &str = "
SELECT assignment, forge, external_id, run_id, owner_id, expires_at_ms FROM leases
WHERE assignment = ?1 AND expires_at_ms > ?2";

pub(super) const LIVE_LEASES: &str = "
SELECT COUNT(*) FROM leases
WHERE assignment = ?1 AND expires_at_ms > ?2";

pub(super) const LIVE_LEASES_TOTAL: &str = "
SELECT COUNT(*) FROM leases
WHERE expires_at_ms > ?1";

pub(super) const RUNS_SINCE: &str = "
SELECT COUNT(*) FROM runs
WHERE assignment = ?1 AND started_at_ms > ?2";

pub(super) const COST_SINCE: &str = "
SELECT COALESCE(SUM(cost_usd), 0.0) FROM runs
WHERE assignment = ?1 AND started_at_ms > ?2";

pub(super) const RECORD_RUN: &str = "
INSERT INTO runs (run_id, assignment, started_at_ms, cost_usd)
VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(run_id) DO NOTHING";

const MIGRATE_RUNS: &str = "
BEGIN;
ALTER TABLE runs RENAME TO runs_legacy;
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY,
    assignment TEXT NOT NULL,
    started_at_ms INTEGER NOT NULL,
    cost_usd REAL NOT NULL
);
INSERT INTO runs (run_id, assignment, started_at_ms, cost_usd)
SELECT 'legacy-' || rowid, assignment, started_at_ms, cost_usd FROM runs_legacy;
DROP TABLE runs_legacy;
COMMIT;
";

pub(super) const SEEN: &str = "SELECT EXISTS(SELECT 1 FROM dedup WHERE content_hash = ?1)";

/// Reads back the stored disposition token, when the hash has one.
pub(super) const DISPOSITION: &str = "SELECT disposition FROM dedup WHERE content_hash = ?1";

/// Writes the marker. On conflict the row updates only when its stored
/// disposition is not the terminal `?4`, so a rejection is never
/// overwritten by a later write; anything weaker still flips.
pub(super) const MARK_SEEN: &str = "
INSERT INTO dedup (content_hash, disposition, at_ms)
VALUES (?1, ?2, ?3)
ON CONFLICT(content_hash) DO UPDATE SET
    disposition = excluded.disposition,
    at_ms = excluded.at_ms
WHERE dedup.disposition != ?4";

/// Maps one `leases` row to a [`Lease`].
pub(super) fn lease_from_row(row: &Row<'_>) -> rusqlite::Result<Lease> {
    let expires: i64 = row.get(5)?;
    Ok(Lease {
        assignment: row.get(0)?,
        forge: row.get(1)?,
        external_id: row.get(2)?,
        run_id: row.get(3)?,
        owner_id: row.get(4)?,
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

/// The stored disposition token for a content hash, when present.
pub(super) fn disposition(conn: &Connection, hash: &str) -> Result<Option<String>, Error> {
    let token = conn
        .query_row(DISPOSITION, (hash,), |row| row.get(0))
        .optional()?;
    Ok(token)
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

/// Adds the v0.2 run id key without discarding legacy budget history.
pub(super) fn migrate_runs(conn: &Connection) -> Result<(), Error> {
    let mut statement = conn.prepare("PRAGMA table_info(runs)")?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    let has_run_id = names.filter_map(Result::ok).any(|name| name == "run_id");
    if !has_run_id {
        conn.execute_batch(MIGRATE_RUNS)?;
    }
    Ok(())
}

const MIGRATE_LEASES_LEGACY: &str = "
BEGIN;
ALTER TABLE leases RENAME TO leases_legacy;
CREATE TABLE leases (
    assignment TEXT NOT NULL,
    forge TEXT NOT NULL,
    external_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    UNIQUE (assignment, forge, external_id)
);
INSERT INTO leases (assignment, forge, external_id, run_id, owner_id, expires_at_ms)
SELECT assignment, forge, external_id, 'legacy-' || rowid, 'legacy-' || rowid, expires_at_ms
FROM leases_legacy;
DROP TABLE leases_legacy;
COMMIT;
";

const MIGRATE_LEASE_OWNERS: &str = "
BEGIN;
ALTER TABLE leases RENAME TO leases_legacy;
CREATE TABLE leases (
    assignment TEXT NOT NULL,
    forge TEXT NOT NULL,
    external_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    UNIQUE (assignment, forge, external_id)
);
INSERT INTO leases (assignment, forge, external_id, run_id, owner_id, expires_at_ms)
SELECT assignment, forge, external_id, run_id, run_id, expires_at_ms FROM leases_legacy;
DROP TABLE leases_legacy;
COMMIT;
";

pub(super) fn migrate_leases(conn: &Connection) -> Result<(), Error> {
    let mut statement = conn.prepare("PRAGMA table_info(leases)")?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    let names: Vec<String> = names.filter_map(Result::ok).collect();
    let has_run_id = names.iter().any(|name| name == "run_id");
    let has_owner_id = names.iter().any(|name| name == "owner_id");
    if !has_run_id {
        conn.execute_batch(MIGRATE_LEASES_LEGACY)?;
    } else if !has_owner_id {
        conn.execute_batch(MIGRATE_LEASE_OWNERS)?;
    }
    Ok(())
}
