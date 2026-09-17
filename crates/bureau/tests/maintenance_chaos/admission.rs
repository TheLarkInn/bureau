use std::future::Future as _;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use bureau::reconcile::{Error, Started};
use bureau::state::Disposition;

use super::fixture::{ASSIGNMENT, Fixture, item};

fn ready(result: Poll<Result<Vec<Started>, Error>>) -> Vec<Started> {
    match result {
        Poll::Ready(result) => result.expect("reconcile pass"),
        Poll::Pending => panic!("the released observation must complete synchronously"),
    }
}

fn competing_passes(fixture: &Fixture) -> Vec<Started> {
    let mut one = pin!(fixture.one.reconcile_once());
    let mut two = pin!(fixture.two.reconcile_once());
    let mut context = Context::from_waker(Waker::noop());
    assert!(
        one.as_mut().poll(&mut context).is_pending(),
        "the first pass must stop after observing the assignment's headroom"
    );
    // Poll directly so both stale snapshots are consumed without scheduling engine tasks.
    let mut started = ready(two.as_mut().poll(&mut context));
    started.extend(ready(one.as_mut().poll(&mut context)));
    started
}

fn preclaimed(fixture: &Fixture, seed: u32) -> u32 {
    let count = (seed / 3) % fixture.limit;
    for id in 0..count {
        let won = fixture
            .one
            .state
            .try_claim(
                ASSIGNMENT,
                "github",
                &format!("held-{id}"),
                Duration::from_secs(60),
            )
            .expect("existing claim");
        assert!(won, "fixture claim must be new");
    }
    fixture
        .one
        .state
        .mark_seen(&item(seed % 2).content_hash(), Disposition::NoChange)
        .expect("dedup fixture");
    count
}

async fn cancel_unpolled(started: Vec<Started>) {
    for run in &started {
        run.handle.abort();
    }
    for run in started {
        let error = run.handle.await.expect_err("unpolled run must be cancelled");
        assert!(error.is_cancelled(), "run must never execute");
        run.owner.expect("claimed owner").release().expect("release");
    }
}

pub(super) async fn shared_assignment_limit(seed: u32) {
    let fixture = Fixture::new(seed);
    let existing = preclaimed(&fixture, seed);
    let started = competing_passes(&fixture);
    let claimed = (
        started.len(),
        fixture.one.state.budget(ASSIGNMENT).expect("budget").live_leases,
    );
    cancel_unpolled(started).await;
    let expected = usize::try_from(fixture.limit - existing).expect("small limit");
    assert_eq!(
        claimed,
        (expected, fixture.limit),
        "seed state={seed}: independent shared-store reconcilers must share capacity"
    );
}
