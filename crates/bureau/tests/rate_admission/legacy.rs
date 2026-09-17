use std::path::Path;

use bureau::state::Store;
use rusqlite::Connection;

use super::support::{ASSIGNMENT, TestDir};

fn seed_legacy(path: &Path) {
    let conn = Connection::open(path).expect("legacy database");
    conn.execute_batch(
        "CREATE TABLE runs (
            assignment TEXT NOT NULL, started_at_ms INTEGER NOT NULL, cost_usd REAL NOT NULL
        );
        CREATE TABLE leases (
            assignment TEXT NOT NULL, forge TEXT NOT NULL, external_id TEXT NOT NULL,
            expires_at_ms INTEGER NOT NULL, UNIQUE (assignment, forge, external_id)
        );",
    )
    .expect("legacy schema without run identities");
    conn.execute(
        "INSERT INTO runs VALUES (?1, 9999999999999, 3.0)",
        [ASSIGNMENT],
    )
    .expect("existing completed counter");
    conn.execute(
        "INSERT INTO leases VALUES (?1, 'github', 'unfinished', 0)",
        [ASSIGNMENT],
    )
    .expect("undated expired attempt");
}

#[test]
fn ancient_completed_and_expired_attempts_both_survive_upgrade() {
    let directory = TestDir::new("ancient");
    let path = directory.path().join("state.db");
    seed_legacy(&path);
    let store = Store::open(&path).expect("upgrade");
    store
        .record_run("legacy-1", ASSIGNMENT, 9.0)
        .expect("replayed completion");
    let budget = store.budget(ASSIGNMENT).expect("budget");
    assert_eq!(
        (
            budget.live_leases,
            budget.runs_this_hour,
            budget.runs_today,
            budget.spent_today_usd
        ),
        (0, 2, 2, 3.0)
    );
}

#[test]
fn read_only_legacy_budget_is_conservative_and_never_migrates() {
    let directory = TestDir::new("read-only");
    let path = directory.path().join("state.db");
    seed_legacy(&path);
    let before = std::fs::read(&path).expect("original bytes");
    let reader = Store::open_read_only(&path).expect("legacy reader");
    let budget = reader.budget(ASSIGNMENT).expect("conservative budget");
    assert_eq!(
        (
            budget.runs_this_hour,
            budget.runs_today,
            std::fs::read(&path).expect("unchanged bytes") == before
        ),
        (2, 2, true)
    );
}

#[test]
fn an_unknown_schema_version_is_not_accepted_by_a_reader_or_writer() {
    let directory = TestDir::new("newer-version");
    let path = directory.path().join("state.db");
    seed_legacy(&path);
    Connection::open(&path)
        .expect("version fixture")
        .pragma_update(None, "user_version", 2)
        .expect("newer version");
    let before = std::fs::read(&path).expect("original bytes");
    let refused = (
        Store::open_read_only(&path).is_err(),
        Store::open(&path).is_err(),
    );
    assert_eq!(
        (
            refused,
            std::fs::read(&path).expect("retained bytes") == before
        ),
        ((true, true), true)
    );
}
