//! State-store adversarial edges (DESIGN.md layer 5): zero-TTL leases,
//! limit boundaries, disposition overwrites, and no-op releases.

use std::path::Path;
use std::time::Duration;

use bureau::config::Limits;
use bureau::state::{Disposition, Store};

use super::testdir::TestDir;

const ASSIGNMENT: &str = "fix-flaky-tests";
const HOUR: Duration = Duration::from_secs(3600);

const fn limits(concurrent: u32, hour: u32, day: u32, prs: u32, cost: f64) -> Limits {
    Limits {
        max_concurrent: Some(concurrent),
        max_runs_per_hour: Some(hour),
        max_runs_per_day: Some(day),
        max_open_prs: Some(prs),
        max_cost_per_day_usd: Some(cost),
        max_run_hours: None,
    }
}

fn claim(store: &Store, external_id: &str, ttl: Duration) -> bool {
    store
        .try_claim(ASSIGNMENT, "github", external_id, ttl)
        .expect("claim")
}

fn headroom(store: &Store, limits: &Limits, open_prs: usize) -> usize {
    store
        .headroom(ASSIGNMENT, limits, open_prs)
        .expect("headroom")
}

fn recorded(store: &Store, runs: u32, cost_usd: f64) {
    for run in 0..runs {
        store
            .record_run(&format!("edge-run-{run}"), ASSIGNMENT, cost_usd)
            .expect("record run");
    }
}

/// Reads the stored disposition straight from `SQLite`, bypassing the
/// store's typed view.
fn disposition_of(path: &Path, hash: &str) -> Option<String> {
    let conn = rusqlite::Connection::open(path).expect("open db");
    conn.query_row(
        "SELECT disposition FROM dedup WHERE content_hash = ?1",
        [hash],
        |row| row.get(0),
    )
    .ok()
}

#[test]
fn a_zero_ttl_lease_is_dead_on_arrival() {
    let store = Store::open_in_memory().expect("open");
    // Expiry equals the insert time, so the next claim's reaper
    // (`expires <= now`) collects it even within the same millisecond.
    let outcome = (
        claim(&store, "item-1", Duration::ZERO),
        store.active(ASSIGNMENT).expect("active").len(),
        store.renew(ASSIGNMENT, "item-1", HOUR).expect("renew"),
        claim(&store, "item-1", HOUR),
    );
    assert_eq!(outcome, (true, 0, false, true));
}

#[test]
fn headroom_at_limit_minus_one_leaves_exactly_one() {
    let by_concurrency = Store::open_in_memory().expect("open");
    assert!(claim(&by_concurrency, "item-1", HOUR));
    let by_hour = Store::open_in_memory().expect("open");
    recorded(&by_hour, 5, 1.0);
    let by_cost = Store::open_in_memory().expect("open");
    recorded(&by_cost, 1, 24.99);
    let idle = Store::open_in_memory().expect("open");
    let outcome = (
        headroom(&by_concurrency, &limits(2, 9, 9, 9, 25.0), 0),
        headroom(&by_hour, &limits(9, 6, 9, 9, 25.0), 0),
        headroom(&idle, &limits(9, 9, 9, 5, 25.0), 4),
        headroom(&by_cost, &limits(3, 9, 9, 9, 25.0), 0),
    );
    assert_eq!(outcome, (1, 1, 1, 3));
}

#[test]
fn a_rejected_marker_is_terminal() {
    let dir = TestDir::new("dedup");
    let path = dir.path().join("state.db");
    let store = Store::open(&path).expect("open");
    store
        .mark_seen("hash-1", Disposition::Rejected)
        .expect("mark");
    store
        .mark_seen("hash-1", Disposition::Proposed)
        .expect("remark");
    // The upsert's conflict clause refuses to overwrite a rejection
    // (DESIGN.md layer 5: "previously rejected" stays out), so the
    // audit trail survives. A weaker marker would still flip.
    let stored = disposition_of(&path, "hash-1");
    assert_eq!(stored.as_deref(), Some("rejected"));
    assert!(store.seen("hash-1").expect("seen"));
}

#[test]
fn releasing_a_never_claimed_key_is_a_noop() {
    let store = Store::open_in_memory().expect("open");
    assert!(claim(&store, "item-1", HOUR));
    let outcome = (
        store.release(ASSIGNMENT, "ghost").is_ok(),
        store.release("other-assignment", "item-1").is_ok(),
        store.active(ASSIGNMENT).expect("active").len(),
        claim(&store, "ghost", HOUR),
    );
    assert_eq!(outcome, (true, true, 1, true));
}
