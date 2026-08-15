//! Reconcile-loop tests (DESIGN.md section 8): pending subtraction,
//! budget gating, dedup, CAS loss, lease release, idempotency, and the
//! wake-driven loop — offline against a local git repo and fake forge.

#[path = "reconcile/world.rs"]
mod world;

use std::time::Duration;

use bureau::config::Limits;
use bureau::contract::StepOutcome;
use bureau::state::Disposition;
use world::{World, generous, item};

#[tokio::test]
async fn pending_subtracts_open_prs_and_active_leases() {
    let world = World::new(&["1", "2", "3"], "true", generous());
    world.open_pr("1").await;
    world.claim("2");
    let started = world.pass().await;
    let claimed = vec!["2".to_owned(), "3".to_owned()];
    assert_eq!((started.len(), world.leased()), (1, claimed));
}

#[tokio::test]
async fn zero_headroom_starts_nothing_and_claims_nothing() {
    let limits = Limits {
        max_open_prs: Some(0),
        ..generous()
    };
    let world = World::new(&["1"], "true", limits);
    let started = world.pass().await;
    assert_eq!((started.len(), world.leased().len()), (0, 0));
}

#[tokio::test]
async fn a_seen_item_is_released_without_a_run() {
    let world = World::new(&["1"], "true", generous());
    let hash = item("1").content_hash();
    world
        .store
        .mark_seen(&hash, Disposition::Proposed)
        .expect("mark_seen");
    let started = world.pass().await;
    assert_eq!((started.len(), world.leased().len()), (0, 0));
}

#[tokio::test]
async fn a_lost_cas_starts_nothing() {
    let world = World::new(&["1"], "true", generous());
    world.claim("1");
    let started = world.pass().await;
    assert_eq!(started.len(), 0);
}

#[tokio::test]
async fn a_finished_run_releases_its_lease() {
    let world = World::new(&["1"], "true", generous());
    let mut outcomes = Vec::new();
    for run in world.pass().await {
        outcomes.push(run.handle.await.expect("run joins"));
    }
    assert_eq!(
        (outcomes.len(), outcomes[0].outcome, world.leased().len()),
        (1, StepOutcome::NoWork, 0)
    );
}

#[tokio::test]
async fn a_second_pass_starts_nothing_new() {
    let world = World::new(&["1"], "echo changed >> file.txt", generous());
    for run in world.pass().await {
        run.handle.await.expect("run joins");
    }
    let second = world.pass().await;
    let observed = world.observed_prs().await;
    assert_eq!((second.len(), world.leased().len(), observed), (0, 0, 1));
}

#[tokio::test]
async fn a_no_work_run_marks_the_item_seen() {
    let world = World::new(&["1"], "true", generous());
    for run in world.pass().await {
        run.handle.await.expect("run joins");
    }
    let hash = item("1").content_hash();
    let marker = world.store.disposition(&hash).expect("disposition");
    let second = world.pass().await;
    let state = (marker, second.len(), world.leased().len());
    assert_eq!(state, (Some(Disposition::NoChange), 0, 0));
}

#[tokio::test]
async fn run_loop_passes_on_wake_and_interval() {
    let world = World::new(&["1"], "true", generous());
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let reconciler = world.reconciler.clone();
    let task = tokio::spawn(async move {
        reconciler.run_loop(Duration::from_millis(50), rx).await;
    });
    tx.send(()).await.expect("wake sends");
    tokio::time::sleep(Duration::from_millis(200)).await;
    // The loop never returns; aborting its task ends the test.
    task.abort();
    assert!(world.run_dirs() >= 1, "at least one pass started a run");
}
