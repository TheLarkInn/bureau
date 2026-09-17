use std::future::Future as _;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use bureau::config::Limits;
use bureau::reconcile::{Error, Started};
use bureau::state::Disposition;

use super::fixture::{ASSIGNMENT, Fixture, item};

fn ready(result: Poll<Result<Vec<Started>, Error>>) -> Result<Vec<Started>, Error> {
    match result {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("the released observation must complete synchronously"),
    }
}

fn collect(passes: [Result<Vec<Started>, Error>; 2]) -> (Vec<Started>, Vec<Error>) {
    let (mut started, mut errors) = (Vec::new(), Vec::new());
    for result in passes {
        match result {
            Ok(mut runs) => started.append(&mut runs),
            Err(error) => errors.push(error),
        }
    }
    (started, errors)
}

fn competing_passes(fixture: &Fixture, between: impl FnOnce()) -> (Vec<Started>, Vec<Error>) {
    let mut one = pin!(fixture.one.reconcile_once());
    let mut two = pin!(fixture.two.reconcile_once());
    let mut context = Context::from_waker(Waker::noop());
    assert!(
        one.as_mut().poll(&mut context).is_pending(),
        "first observation"
    );
    assert!(
        two.as_mut().poll(&mut context).is_pending(),
        "second observation"
    );
    between();
    let mut release = pin!(fixture.barrier.wait());
    assert!(
        release.as_mut().poll(&mut context).is_ready(),
        "release observations"
    );
    // Poll directly so both stale snapshots are consumed without scheduling engine tasks.
    collect([
        ready(two.as_mut().poll(&mut context)),
        ready(one.as_mut().poll(&mut context)),
    ])
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

async fn cancelled(run: Started) {
    let error = run
        .handle
        .await
        .expect_err("unpolled run must be cancelled");
    assert!(error.is_cancelled(), "run must never execute");
    run.owner
        .expect("claimed owner")
        .release()
        .expect("release");
}

async fn cancel_unpolled(started: Vec<Started>) {
    for run in &started {
        run.handle.abort();
    }
    for run in started {
        cancelled(run).await;
    }
}

async fn cancel_unpublished(fixture: &Fixture, started: Vec<Started>) -> usize {
    let directories: Vec<_> = started
        .iter()
        .map(|run| fixture.one.engine.runs_dir.join(&run.run_id))
        .collect();
    let count = directories.len();
    cancel_unpolled(started).await;
    // Spawn creates the directory synchronously; never erase a log if a task actually ran.
    for directory in directories {
        std::fs::remove_dir(directory).expect("cancelled unpolled run directory must be empty");
    }
    count
}

async fn shared_assignment_limit(seed: u32) {
    let fixture = Fixture::new(seed);
    let existing = preclaimed(&fixture, seed);
    let (started, failed) = competing_passes(&fixture, || {});
    let claimed = (
        started.len(),
        fixture
            .one
            .state
            .budget(ASSIGNMENT)
            .expect("budget")
            .live_leases,
        failed.len(),
    );
    cancel_unpolled(started).await;
    let expected = usize::try_from(fixture.limit - existing).expect("small limit");
    assert_eq!(
        claimed,
        (expected, fixture.limit, 0),
        "seed state={seed}: independent shared-store reconcilers must share capacity"
    );
}

fn exhausted_limit(seed: u32) -> Limits {
    match seed % 3 {
        0 => Limits {
            max_runs_per_hour: Some(1),
            ..Limits::default()
        },
        1 => Limits {
            max_runs_per_day: Some(1),
            ..Limits::default()
        },
        _ => Limits {
            max_cost_per_day_usd: Some(1.0),
            ..Limits::default()
        },
    }
}

async fn recorded_budget(seed: u32) {
    let mut fixture = Fixture::new(seed);
    fixture.set_limits(&exhausted_limit(seed));
    let (started, failed) = competing_passes(&fixture, || {
        fixture
            .one
            .state
            .record_run("completed-elsewhere", ASSIGNMENT, 1.0)
            .expect("exhaust budget after both observations");
    });
    let count = started.len();
    cancel_unpolled(started).await;
    assert_eq!(
        (
            count,
            failed.len(),
            fixture.one.state.active(ASSIGNMENT).expect("leases").len()
        ),
        (0, 0, 0),
        "state={seed}: current recorded budget must override stale headroom"
    );
}

async fn unavailable_budget(seed: u32) {
    let fixture = Fixture::new(seed);
    let (started, failed) = competing_passes(&fixture, || {
        rusqlite::Connection::open(fixture.database())
            .expect("failure injection")
            .execute("DROP TABLE runs", [])
            .expect("inject unavailable budget");
    });
    let count = started.len();
    cancel_unpolled(started).await;
    let messages: Vec<String> = failed.iter().map(ToString::to_string).collect();
    assert_eq!(
        (
            count,
            failed.len(),
            fixture.one.state.active(ASSIGNMENT).expect("leases").len()
        ),
        (0, 2, 0),
        "state={seed}: budget errors must surface without claims: {messages:?}"
    );
}

async fn unprojected_rate_budget(seed: u32) {
    let mut fixture = Fixture::new(seed);
    fixture.set_limits(&exhausted_limit(seed % 2));
    let (first, first_errors) = competing_passes(&fixture, || {});
    let count = cancel_unpublished(&fixture, first).await;
    let (second, second_errors) = competing_passes(&fixture, || {});
    let repeated = second.len();
    cancel_unpolled(second).await;
    let budget = fixture
        .one
        .state
        .budget(ASSIGNMENT)
        .expect("admitted budget");
    assert_eq!(
        (
            count,
            repeated,
            first_errors.len() + second_errors.len(),
            budget.runs_this_hour,
            budget.runs_today
        ),
        (1, 0, 0, 1, 1),
        "state={seed}: releasing an unprojected run must not reopen rate capacity"
    );
}

pub async fn check(seed: u32) {
    shared_assignment_limit(seed).await;
    recorded_budget(seed).await;
    unavailable_budget(seed).await;
    unprojected_rate_budget(seed).await;
}
