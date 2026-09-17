//! Preserves legacy run and lease identities before admission accounting is established.

use rusqlite::Connection;

use super::Error;

const RUNS: &str = "
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

const LEASES: &str = "
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

const OWNERS: &str = "
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

fn columns(conn: &Connection, table: &str) -> Result<Vec<String>, Error> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    Ok(names.collect::<Result<_, _>>()?)
}

pub(super) fn runs(conn: &Connection) -> Result<(), Error> {
    if !columns(conn, "runs")?.iter().any(|name| name == "run_id") {
        conn.execute_batch(RUNS)?;
    }
    Ok(())
}

pub(super) fn leases(conn: &Connection) -> Result<(), Error> {
    let names = columns(conn, "leases")?;
    let has_run_id = names.iter().any(|name| name == "run_id");
    let has_owner_id = names.iter().any(|name| name == "owner_id");
    if !has_run_id {
        conn.execute_batch(LEASES)?;
    } else if !has_owner_id {
        conn.execute_batch(OWNERS)?;
    }
    Ok(())
}
