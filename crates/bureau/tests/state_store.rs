//! Layer 5 state-store contract tests: offline, no network, no model
//! calls (DESIGN.md sections 7 and 12). The two-daemon case runs two
//! `Store` instances against one database file.

use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use bureau::config::Limits;
use bureau::state::{Disposition, Store};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-state-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const ASSIGNMENT: &str = "fix-flaky-tests";
const HOUR: Duration = Duration::from_secs(3600);

const fn limits(concurrent: u32, hour: u32, day: u32, prs: u32, cost: f64) -> Limits {
    Limits {
        max_concurrent: concurrent,
        max_runs_per_hour: hour,
        max_runs_per_day: day,
        max_open_prs: prs,
        max_cost_per_day_usd: cost,
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
    for _ in 0..runs {
        store.record_run(ASSIGNMENT, cost_usd).expect("record run");
    }
}

#[test]
fn claim_succeeds_once_per_key() {
    let store = Store::open_in_memory().expect("open");
    let outcome = (
        claim(&store, "item-1", HOUR),
        claim(&store, "item-1", HOUR),
        claim(&store, "item-2", HOUR),
    );
    assert_eq!(outcome, (true, false, true));
}

#[test]
fn expired_lease_is_reclaimable() {
    let store = Store::open_in_memory().expect("open");
    let first = claim(&store, "item-1", Duration::from_millis(50));
    thread::sleep(Duration::from_millis(120));
    let reclaimed = claim(&store, "item-1", HOUR);
    assert_eq!((first, reclaimed), (true, true));
}

#[test]
fn renew_extends_a_live_lease() {
    let store = Store::open_in_memory().expect("open");
    assert!(claim(&store, "item-1", Duration::from_millis(300)));
    thread::sleep(Duration::from_millis(100));
    let renewed = store
        .renew(ASSIGNMENT, "item-1", Duration::from_millis(800))
        .expect("renew");
    thread::sleep(Duration::from_millis(350));
    let live = store.active(ASSIGNMENT).expect("active").len();
    assert_eq!((renewed, live), (true, 1), "past the original expiry");
}

#[test]
fn renew_fails_on_expired_or_missing_lease() {
    let store = Store::open_in_memory().expect("open");
    assert!(claim(&store, "item-1", Duration::from_millis(50)));
    thread::sleep(Duration::from_millis(120));
    let outcome = (
        store.renew(ASSIGNMENT, "item-1", HOUR).expect("renew"),
        store.renew(ASSIGNMENT, "item-2", HOUR).expect("renew"),
    );
    assert_eq!(outcome, (false, false));
}

#[test]
fn release_is_idempotent() {
    let store = Store::open_in_memory().expect("open");
    assert!(claim(&store, "item-1", HOUR));
    let outcome = (
        store.release(ASSIGNMENT, "item-1").is_ok(),
        store.release(ASSIGNMENT, "item-1").is_ok(),
        claim(&store, "item-1", HOUR),
    );
    assert_eq!(outcome, (true, true, true));
}

#[test]
fn active_filters_expired_leases() {
    let store = Store::open_in_memory().expect("open");
    assert!(claim(&store, "short", Duration::from_millis(50)));
    assert!(claim(&store, "long", HOUR));
    thread::sleep(Duration::from_millis(120));
    let ids: Vec<String> = store
        .active(ASSIGNMENT)
        .expect("active")
        .into_iter()
        .map(|lease| lease.external_id)
        .collect();
    assert_eq!(ids, ["long"]);
}

/// One claim attempt per round; the barrier guarantees both stores race
/// every round, and the release clears the key for the next one.
fn claim_rounds(store: &Store, barrier: &Barrier, rounds: u32) -> u32 {
    let mut wins = 0;
    for _ in 0..rounds {
        barrier.wait();
        if store
            .try_claim(ASSIGNMENT, "github", "item-1", HOUR)
            .expect("claim")
        {
            wins += 1;
        }
        barrier.wait();
        store.release(ASSIGNMENT, "item-1").expect("release");
    }
    wins
}

#[test]
fn two_stores_arbitrate_through_the_database() {
    let dir = TestDir::new("cas");
    let path = dir.path().join("state.db");
    let one = Store::open(&path).expect("open one");
    let two = Store::open(&path).expect("open two");
    let barrier = Barrier::new(2);
    let wins = thread::scope(|scope| {
        let first = scope.spawn(|| claim_rounds(&one, &barrier, 50));
        let second = scope.spawn(|| claim_rounds(&two, &barrier, 50));
        first.join().expect("join one") + second.join().expect("join two")
    });
    assert_eq!(wins, 50, "every round must have exactly one winner");
}

#[test]
fn state_survives_reopen() {
    let dir = TestDir::new("reopen");
    let path = dir.path().join("nested").join("state.db");
    let store = Store::open(&path).expect("open");
    assert!(claim(&store, "item-1", HOUR));
    store
        .mark_seen("hash-1", Disposition::Proposed)
        .expect("mark");
    drop(store);
    let reopened = Store::open(&path).expect("reopen");
    let outcome = (
        claim(&reopened, "item-1", HOUR),
        reopened.seen("hash-1").expect("seen"),
    );
    assert_eq!(outcome, (false, true));
}

#[test]
fn headroom_is_the_tightest_limit_when_idle() {
    let store = Store::open_in_memory().expect("open");
    assert_eq!(headroom(&store, &limits(2, 6, 40, 5, 25.0), 0), 2);
}

#[test]
fn headroom_drops_with_live_leases() {
    let store = Store::open_in_memory().expect("open");
    assert!(claim(&store, "item-1", HOUR));
    assert_eq!(headroom(&store, &limits(3, 6, 40, 5, 25.0), 0), 2);
}

#[test]
fn headroom_is_zero_at_concurrent_limit() {
    let store = Store::open_in_memory().expect("open");
    assert!(claim(&store, "item-1", HOUR));
    assert!(claim(&store, "item-2", HOUR));
    assert_eq!(headroom(&store, &limits(2, 6, 40, 5, 25.0), 0), 0);
}

#[test]
fn headroom_counts_runs_started_this_hour() {
    let store = Store::open_in_memory().expect("open");
    recorded(&store, 2, 1.0);
    assert_eq!(headroom(&store, &limits(9, 6, 40, 9, 25.0), 0), 4);
}

#[test]
fn headroom_is_zero_at_hourly_limit() {
    let store = Store::open_in_memory().expect("open");
    recorded(&store, 6, 1.0);
    assert_eq!(headroom(&store, &limits(9, 6, 40, 9, 25.0), 0), 0);
}

#[test]
fn headroom_is_zero_at_daily_limit() {
    let store = Store::open_in_memory().expect("open");
    recorded(&store, 2, 1.0);
    assert_eq!(headroom(&store, &limits(9, 9, 2, 9, 25.0), 0), 0);
}

#[test]
fn headroom_is_zero_at_cost_limit() {
    let store = Store::open_in_memory().expect("open");
    recorded(&store, 1, 25.0);
    assert_eq!(headroom(&store, &limits(9, 9, 9, 9, 25.0), 0), 0);
}

#[test]
fn headroom_counts_open_prs_from_the_forge() {
    let store = Store::open_in_memory().expect("open");
    let outcome = (
        headroom(&store, &limits(9, 9, 9, 5, 25.0), 3),
        headroom(&store, &limits(9, 9, 9, 5, 25.0), 5),
    );
    assert_eq!(outcome, (2, 0));
}

#[test]
fn dedup_marks_once_and_is_idempotent() {
    let store = Store::open_in_memory().expect("open");
    let before = store.seen("hash-1").expect("seen");
    store
        .mark_seen("hash-1", Disposition::Proposed)
        .expect("mark");
    let after = store.seen("hash-1").expect("seen");
    store
        .mark_seen("hash-1", Disposition::Rejected)
        .expect("remark");
    let still = store.seen("hash-1").expect("seen");
    assert_eq!((before, after, still), (false, true, true));
}
