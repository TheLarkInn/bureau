//! The wired-path behavior port (goober's `test/e2e/integration_test.go`
//! and `engine_start_project_test.go`): committed config loads through
//! the real loader, a reconcile pass starts the run, the engine drives
//! it to a PR with the shipped plugin's agents pinned, and the run log
//! projects to identical state with idempotent re-projection.

#[path = "behavior/wired_support.rs"]
mod support;

use bureau::contract::StepOutcome;
use bureau::engine;
use bureau::forge::Forge as _;
use bureau::runlog::{self, RunState, RunStatus};
use support::World;

/// The wired path (goober's `TestWalkingSkeletonWiredPath` port): config
/// on disk -> load -> reconcile -> engine -> PR, with plugin-referenced
/// roles resolved and pinned through the run.
#[tokio::test]
async fn committed_config_drives_a_reconcile_pass_to_a_pr() {
    let world = World::new();
    let outcomes = world.pass_and_join().await;
    let prs = world
        .forge
        .open_prs(&world.primary_url(), "bureau/fix/")
        .await
        .expect("open_prs");
    let state = (
        outcomes.len(),
        outcomes[0].outcome,
        prs.len(),
        prs[0].item_id.as_deref(),
    );
    assert_eq!(state, (1, StepOutcome::Success, 1, Some("101")));
    check_pinned_snapshot(&world, &outcomes[0].run_id);
}

/// The run pinned its committed config source and the resolved bureau
/// plugin's digest — the run's exact inputs are recoverable from the
/// log alone.
fn check_pinned_snapshot(world: &World, run_id: &str) {
    let dir = world.dir.path().join("runs").join(run_id);
    let snapshot = runlog::replay_state(&dir)
        .expect("replay")
        .snapshot
        .expect("snapshot");
    let pinned = (
        snapshot.config_source.expect("config source").commit,
        snapshot
            .plugin_sources
            .get("bureau")
            .map(|source| !source.digest.is_empty()),
    );
    assert_eq!(
        pinned,
        (
            "0000000000000000000000000000000000000000".to_owned(),
            Some(true)
        )
    );
}

/// The projection round trip (goober's engine-start/project port): the
/// derived state cache equals the replayed log, deleting it loses
/// nothing, and re-running a finished run appends nothing.
#[tokio::test]
async fn a_finished_run_projects_identically_and_reprojects_as_a_noop() {
    let world = World::new();
    let outcomes = world.pass_and_join().await;
    let run_id = &outcomes[0].run_id;
    let dir = world.dir.path().join("runs").join(run_id);
    check_projection(&dir);
    check_idempotent_rerun(&world, &dir).await;
}

/// state.json is the replay of events.jsonl, even after it is deleted.
fn check_projection(dir: &std::path::Path) {
    let cached: RunState =
        serde_json::from_slice(&std::fs::read(dir.join(runlog::STATE_FILE)).expect("state cache"))
            .expect("state parses");
    let replayed = runlog::replay_state(dir).expect("state replays");
    assert_eq!(cached, replayed, "cache is the log's projection");
    std::fs::remove_file(dir.join(runlog::STATE_FILE)).expect("delete cache");
    let rebuilt = runlog::replay_state(dir).expect("replay without cache");
    assert_eq!(rebuilt.status, RunStatus::Finished(StepOutcome::Success));
    assert_eq!(rebuilt, cached, "the log alone reconstructs the state");
}

/// Re-running the finished run's rehydrated plan returns the recorded
/// outcome and appends nothing — the re-projection no-op.
async fn check_idempotent_rerun(world: &World, dir: &std::path::Path) {
    let before = runlog::read_events(dir).expect("events").len();
    let snapshot = runlog::replay_state(dir)
        .expect("replay")
        .snapshot
        .expect("snapshot");
    let credentials = world.reconciler.credentials.clone();
    let plan = engine::rehydrate(snapshot, world.forge.clone(), credentials);
    let second = world.engine.run(&plan).await;
    let after = runlog::read_events(dir).expect("events").len();
    assert_eq!(
        (second.outcome, after),
        (StepOutcome::Success, before),
        "a finished run resumes to its recorded outcome"
    );
}
