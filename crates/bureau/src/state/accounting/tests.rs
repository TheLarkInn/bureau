use rusqlite::Connection;

use super::super::{DAY_MS, HOUR_MS, Store, sql};
use super::{migrate, record, runs_since, schema_ready};

const ASSIGNMENT: &str = "rate";
const NOW: i64 = 3 * DAY_MS;

fn store() -> Store {
    eprintln!("BUREAU_CHAOS_SEED=0 admission accounting fixture");
    Store::open_in_memory().expect("store")
}

fn legacy() -> Connection {
    eprintln!("BUREAU_CHAOS_SEED=0 legacy accounting fixture");
    let conn = Connection::open_in_memory().expect("legacy database");
    conn.execute_batch(sql::SCHEMA).expect("legacy schema");
    conn
}

fn admit(conn: &Connection, run: &str, at: i64) {
    record(conn, ASSIGNMENT, "github", "item", run, at).expect("admission");
}

fn completed(conn: &Connection, run: &str, at: i64) {
    conn.execute(sql::RECORD_RUN, (run, ASSIGNMENT, at, 3.0))
        .expect("terminal record");
}

fn retained(conn: &Connection, run: &str) {
    conn.execute(
        sql::INSERT_LEASE,
        (ASSIGNMENT, "github", "item", run, "generation", 0),
    )
    .expect("expired legacy lease");
}

fn admitted_at(conn: &Connection) -> i64 {
    conn.query_row("SELECT admitted_at_ms FROM run_admissions", [], |row| {
        row.get(0)
    })
    .expect("recorded admission time")
}

#[test]
fn sliding_rate_windows_have_an_exact_exclusive_lower_boundary() {
    for window in [HOUR_MS, DAY_MS] {
        let store = store();
        let conn = store.lock();
        for (run, offset) in [("before", -1), ("boundary", 0), ("after", 1)] {
            admit(&conn, run, NOW - window + offset);
        }
        assert_eq!(
            runs_since(&conn, ASSIGNMENT, NOW - window).expect("count"),
            1
        );
    }
}

#[test]
fn repeated_admission_preserves_its_original_timestamp() {
    let store = store();
    let conn = store.lock();
    admit(&conn, "run", NOW - HOUR_MS);
    admit(&conn, "run", NOW);
    assert_eq!(
        (
            admitted_at(&conn),
            runs_since(&conn, ASSIGNMENT, NOW - HOUR_MS).expect("count")
        ),
        (NOW - HOUR_MS, 0)
    );
}

#[test]
fn late_terminal_cost_does_not_move_the_admission_into_a_new_window() {
    let store = store();
    let conn = store.lock();
    admit(&conn, "run", NOW - DAY_MS);
    completed(&conn, "run", NOW);
    completed(&conn, "run", NOW);
    assert_eq!(
        (
            runs_since(&conn, ASSIGNMENT, NOW - DAY_MS).expect("count"),
            sql::cost_since(&conn, ASSIGNMENT, NOW - DAY_MS).expect("cost"),
            admitted_at(&conn)
        ),
        (0, 3.0, NOW - DAY_MS)
    );
}

#[test]
fn unrecorded_legacy_completions_retain_their_existing_counters() {
    let store = store();
    let conn = store.lock();
    completed(&conn, "legacy-completed", NOW);
    assert_eq!(
        (
            runs_since(&conn, ASSIGNMENT, NOW - HOUR_MS).expect("count"),
            sql::cost_since(&conn, ASSIGNMENT, NOW - DAY_MS).expect("cost")
        ),
        (1, 3.0)
    );
}

#[test]
fn migration_retains_expired_unfinished_attempts_without_retiming_on_reopen() {
    let mut conn = legacy();
    retained(&conn, "unfinished");
    migrate(&mut conn, || NOW).expect("first migration");
    conn.execute("DELETE FROM leases", [])
        .expect("expired lease removed");
    migrate(&mut conn, || NOW + DAY_MS).expect("reopen");
    assert_eq!(
        (
            admitted_at(&conn),
            runs_since(&conn, ASSIGNMENT, NOW - HOUR_MS).expect("count")
        ),
        (NOW, 1)
    );
}

#[test]
fn known_legacy_terminal_identity_is_not_charged_again() {
    let mut conn = legacy();
    retained(&conn, "known-run");
    completed(&conn, "known-run", NOW - 100);
    migrate(&mut conn, || NOW).expect("migration");
    assert_eq!(
        (
            admitted_at(&conn),
            runs_since(&conn, ASSIGNMENT, NOW - HOUR_MS).expect("count")
        ),
        (NOW - 100, 1)
    );
}

#[test]
fn ambiguous_synthetic_legacy_ids_preserve_both_existing_histories() {
    let mut conn = legacy();
    retained(&conn, "legacy-1");
    completed(&conn, "legacy-1", NOW - 100);
    migrate(&mut conn, || NOW).expect("migration");
    admit(&conn, "legacy-1", NOW + 1);
    completed(&conn, "legacy-1", NOW + 1);
    assert_eq!(
        (
            admitted_at(&conn),
            runs_since(&conn, ASSIGNMENT, NOW - HOUR_MS).expect("count")
        ),
        (NOW, 2)
    );
}

#[test]
fn read_only_legacy_view_counts_undated_leases_without_migrating() {
    let conn = legacy();
    retained(&conn, "unfinished");
    completed(&conn, "completed", NOW);
    conn.pragma_update(None, "query_only", true)
        .expect("read only");
    let count = runs_since(&conn, ASSIGNMENT, NOW - HOUR_MS).expect("conservative view");
    assert_eq!((count, schema_ready(&conn).expect("schema")), (2, false));
}

#[test]
fn missing_accounting_evidence_never_becomes_zero_capacity_usage() {
    for table in ["run_admissions", "runs", "leases"] {
        let store = store();
        store
            .lock()
            .execute(&format!("DROP TABLE {table}"), [])
            .expect("inject loss");
        assert!(store.budget(ASSIGNMENT).is_err(), "missing {table}");
    }
}

#[test]
fn unversioned_or_newer_accounting_is_rejected_without_guessing() {
    let conn = legacy();
    conn.pragma_update(None, "user_version", 2)
        .expect("newer schema");
    assert!(schema_ready(&conn).is_err());
    conn.pragma_update(None, "user_version", 0)
        .expect("unversioned");
    conn.execute_batch(super::SCHEMA)
        .expect("unversioned accounting");
    assert!(schema_ready(&conn).is_err());
}

#[test]
fn counter_overflow_and_negative_values_are_errors_not_zero() {
    for count in [-1, i64::from(u32::MAX) + 1] {
        assert!(
            sql::count_value(count).is_err(),
            "BUREAU_CHAOS_SEED=0 count={count}"
        );
    }
}
