//! Rate-only limits must bound admissions before any terminal event exists.

use std::sync::Barrier;
use std::thread;

use bureau::config::Limits;
use bureau::state::FreshClaim;

#[path = "rate_admission/legacy.rs"]
mod legacy;
#[path = "rate_admission/lifecycle.rs"]
mod lifecycle;
#[path = "rate_admission/projection.rs"]
mod projection;
#[path = "rate_admission/support.rs"]
mod support;

use support::{ASSIGNMENT, Fixture, rate_limits};

fn together(fixture: &Fixture, limits: &Limits) -> [Option<FreshClaim>; 2] {
    let owners = [
        fixture.owner(0, "first", "first-run"),
        fixture.owner(1, "second", "second-run"),
    ];
    let barrier = Barrier::new(2);
    thread::scope(|scope| {
        let one = scope.spawn(|| {
            barrier.wait();
            fixture.claim(&owners[0], limits)
        });
        let two = scope.spawn(|| {
            barrier.wait();
            fixture.claim(&owners[1], limits)
        });
        [
            one.join().expect("first claim"),
            two.join().expect("second claim"),
        ]
    })
}

fn counts(fixture: &Fixture, claims: &[Option<FreshClaim>]) -> (usize, u32, u32, u32) {
    let admitted = claims
        .iter()
        .filter(|claim| matches!(claim, Some(FreshClaim::Claimed)))
        .count();
    let budget = fixture.stores[0]
        .budget(ASSIGNMENT)
        .expect("current budget");
    (
        admitted,
        budget.live_leases,
        budget.runs_this_hour,
        budget.runs_today,
    )
}

#[test]
fn rate_only_limits_fence_competing_fresh_claims() {
    let outcomes: Vec<_> = rate_limits()
        .iter()
        .map(|limits| {
            eprintln!("BUREAU_CHAOS_SEED=0 competing rate-only limits={limits:?}");
            let fixture = Fixture::new("competing");
            counts(&fixture, &together(&fixture, limits))
        })
        .collect();
    assert_eq!(outcomes, [(1, 1, 1, 1); 2], "hour-only, then day-only");
}

fn after_release(limits: &Limits) -> (Option<FreshClaim>, Option<FreshClaim>) {
    eprintln!("BUREAU_CHAOS_SEED=0 sequential rate-only limits={limits:?}");
    let fixture = Fixture::new("sequential");
    let first = fixture.owner(0, "first", "first-run");
    let admitted = fixture.claim(&first, limits);
    first.release().expect("release without a terminal record");
    let second = fixture.owner(1, "second", "second-run");
    (admitted, fixture.claim(&second, limits))
}

#[test]
fn rate_only_limits_survive_release_before_terminal_projection() {
    let outcomes: Vec<_> = rate_limits().iter().map(after_release).collect();
    assert_eq!(
        outcomes,
        [
            (Some(FreshClaim::Claimed), None),
            (Some(FreshClaim::Claimed), None),
        ],
        "hour-only, then day-only"
    );
}
