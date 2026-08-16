//! Idempotent run accounting and legacy state migration.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::Limits;
use bureau::state::Store;
use rusqlite::Connection;

static NEXT: AtomicU32 = AtomicU32::new(0);

#[test]
fn recording_one_run_twice_counts_once() {
    let store = Store::open_in_memory().expect("store");
    store.record_run("run-1", "assignment", 3.0).expect("first");
    store
        .record_run("run-1", "assignment", 3.0)
        .expect("second");
    let limits = Limits {
        max_runs_per_day: Some(2),
        ..Limits::default()
    };
    assert_eq!(
        store.headroom("assignment", &limits, 0).expect("headroom"),
        1
    );
}

#[test]
fn legacy_runs_are_migrated_without_losing_budget_history() {
    let path = temp_db();
    let conn = Connection::open(&path).expect("legacy db");
    conn.execute_batch(
        "CREATE TABLE runs (assignment TEXT NOT NULL, started_at_ms INTEGER NOT NULL, cost_usd REAL NOT NULL);
         INSERT INTO runs VALUES ('assignment', 9999999999999, 3.0);",
    )
    .expect("legacy schema");
    drop(conn);
    let store = Store::open(&path).expect("migrated store");
    let limits = Limits {
        max_runs_per_day: Some(1),
        ..Limits::default()
    };
    assert_eq!(
        store.headroom("assignment", &limits, 0).expect("headroom"),
        0
    );
    std::fs::remove_file(path).expect("cleanup");
}

#[test]
fn run_owned_leases_gain_a_supervisor_generation() {
    let path = temp_db();
    let conn = Connection::open(&path).expect("legacy db");
    conn.execute_batch(
        "CREATE TABLE leases (
            assignment TEXT NOT NULL, forge TEXT NOT NULL, external_id TEXT NOT NULL,
            run_id TEXT NOT NULL, expires_at_ms INTEGER NOT NULL,
            UNIQUE (assignment, forge, external_id)
        );
        INSERT INTO leases VALUES ('assignment', 'github', '42', 'run-1', 9999999999999);",
    )
    .expect("legacy schema");
    drop(conn);
    let store = Store::open(&path).expect("migrated store");
    let lease = store
        .active("assignment")
        .expect("active")
        .pop()
        .expect("lease");
    assert_eq!(
        (lease.run_id, lease.owner_id),
        ("run-1".to_owned(), "run-1".to_owned())
    );
    std::fs::remove_file(path).expect("cleanup");
}

fn temp_db() -> PathBuf {
    std::env::temp_dir().join(format!(
        "bureau-state-migration-{}-{}.db",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
