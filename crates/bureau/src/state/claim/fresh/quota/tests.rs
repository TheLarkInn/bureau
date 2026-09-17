use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::{FreshClaim, LeaseOwner, Limits};
use crate::state::{Disposition, Store};

const TTL: Duration = Duration::from_secs(60);
const ASSIGNMENT: &str = "quota";

struct Fixture {
    store: Arc<Store>,
    runs: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        eprintln!("BUREAU_CHAOS_SEED=0 fenced dedup fixture");
        let id = crate::identity::random_hex().expect("fixture identity");
        Self {
            store: Arc::new(Store::open_in_memory().expect("store")),
            runs: std::env::temp_dir().join(format!("bureau-unpublished-quota-{id}")),
        }
    }

    fn owner(&self, run: &str, hash: &str) -> LeaseOwner {
        LeaseOwner::new(self.store.clone(), ASSIGNMENT, "github", hash, run).expect("owner")
    }

    fn attempt(&self, limits: &Limits, run: &str, hash: &str) -> Option<FreshClaim> {
        let owner = self.owner(run, hash);
        let claim = owner
            .claim_fresh_unseen_with_limits(TTL, &self.runs, limits, 0, hash)
            .expect("fenced unseen admission");
        owner
            .release()
            .expect("release without terminal projection");
        claim
    }
}

fn rate_limits() -> [Limits; 2] {
    [
        Limits {
            max_runs_per_hour: Some(1),
            ..Limits::default()
        },
        Limits {
            max_runs_per_day: Some(1),
            ..Limits::default()
        },
    ]
}

fn repeated_passes(limits: &Limits) -> ([Option<FreshClaim>; 4], u32, u32) {
    let fixture = Fixture::new();
    fixture
        .store
        .mark_seen("seen", Disposition::NoChange)
        .expect("seen item");
    let claims = [
        fixture.attempt(limits, "seen-first", "seen"),
        fixture.attempt(limits, "fresh-first", "fresh"),
        fixture.attempt(limits, "seen-repeat", "seen"),
        fixture.attempt(limits, "fresh-repeat", "fresh"),
    ];
    let budget = fixture.store.budget(ASSIGNMENT).expect("budget");
    (claims, budget.runs_this_hour, budget.runs_today)
}

#[test]
fn seen_content_never_consumes_hour_or_day_capacity_for_later_fresh_work() {
    for limits in rate_limits() {
        assert_eq!(
            repeated_passes(&limits),
            (
                [
                    Some(FreshClaim::Busy),
                    Some(FreshClaim::Claimed),
                    Some(FreshClaim::Busy),
                    None
                ],
                1,
                1
            ),
            "{limits:?}"
        );
    }
}

#[test]
fn explicit_run_or_retry_may_repeat_seen_content_within_its_budget() {
    let fixture = Fixture::new();
    fixture
        .store
        .mark_seen("seen", Disposition::NoChange)
        .expect("seen item");
    let owner = fixture.owner("manual", "seen");
    let claimed = owner
        .claim_fresh_with_limits(TTL, &fixture.runs, &rate_limits()[0], 0)
        .expect("explicit admission");
    let budget = fixture.store.budget(ASSIGNMENT).expect("budget");
    assert_eq!(
        (claimed, budget.runs_this_hour),
        (Some(FreshClaim::Claimed), 1)
    );
}

#[test]
fn a_later_seen_marker_never_refunds_committed_admission_history() {
    let fixture = Fixture::new();
    let claimed = fixture.attempt(&rate_limits()[0], "first", "content");
    fixture
        .store
        .mark_seen("content", Disposition::NoChange)
        .expect("later marker");
    let skipped = fixture.attempt(&rate_limits()[0], "later", "content");
    let budget = fixture.store.budget(ASSIGNMENT).expect("budget");
    assert_eq!(
        (claimed, skipped, budget.runs_this_hour),
        (Some(FreshClaim::Claimed), Some(FreshClaim::Busy), 1)
    );
}

#[test]
fn unreadable_dedup_fails_before_any_lease_or_rate_charge() {
    let fixture = Fixture::new();
    fixture
        .store
        .lock()
        .execute("DROP TABLE dedup", [])
        .expect("inject failure");
    let owner = fixture.owner("new", "content");
    let result =
        owner.claim_fresh_unseen_with_limits(TTL, &fixture.runs, &rate_limits()[0], 0, "content");
    let budget = fixture.store.budget(ASSIGNMENT).expect("budget");
    assert_eq!(
        (result.is_err(), budget.live_leases, budget.runs_this_hour),
        (true, 0, 0)
    );
}
