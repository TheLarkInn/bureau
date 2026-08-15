//! Section 13 definition-of-done proofs (DESIGN.md): killing the daemon
//! mid-run and restarting resumes from `events.jsonl`, and two daemons
//! sharing one state file cannot claim the same item. Offline: local
//! git repos and the fake forge, like the engine and reconcile suites.

#[path = "dod/fixture.rs"]
mod fixture;

use std::path::Path;
use std::time::{Duration, Instant};

use bureau::contract::StepOutcome;
use bureau::engine::{Engine, RunOutcome, RunPlan};
use bureau::runlog::{self, Event, EventKind, RunStatus};
use fixture::{Daemons, Rig, det_step};
use tokio::runtime::Builder;

/// The blocking step's work: change the worktree, then wait out the
/// kill (or the PROCEED marker). Bounded, so even a leaked child dies
/// within a minute.
const BLOCK_RUN: &str = "echo changed >> file.txt; i=0; while [ $i -lt 1200 ] && [ ! -f ../PROCEED ]; do sleep 0.05; i=$((i+1)); done";

/// `first` completes; `block` writes the change then waits for PROCEED.
/// `max_attempts` 2: a `step_started` without `step_finished` consumes
/// one attempt, so the killed step needs a second to re-enter.
fn kill_steps() -> Vec<bureau::config::StepDef> {
    let mut block = det_step("block", BLOCK_RUN, Some("done"));
    block.max_attempts = 2;
    vec![det_step("first", "true", Some("block")), block]
}

/// §13: killing the daemon mid-run and restarting resumes from
/// `events.jsonl` — finished steps are not re-run, the interrupted step
/// is re-entered, and the run still lands its PR. The 1-byte credential
/// keeps the scrubber's holdback at 0 so the kill cannot tear the log's
/// last line; the torn shape is covered by the ignored test below.
#[tokio::test]
async fn killing_the_daemon_mid_run_resumes_from_events_jsonl() {
    let rig = Rig::new();
    let plan = rig.plan("x", kill_steps());
    let resume_plan = plan.clone();
    let run_dir = rig.dir.path().join("runs").join(&plan.run_id);
    kill_mid_run(&rig, plan);
    assert_mid_kill(&run_dir);
    std::fs::write(run_dir.join("PROCEED"), "go\n").expect("PROCEED writes");
    let second = rig.engine().run(&resume_plan).await;
    check_resumed_run(&run_dir, &second);
}

/// A multi-byte credential cannot corrupt JSON keys or leave a scrubber
/// holdback tail: run-log scrubbing operates on complete string values.
#[tokio::test]
async fn a_killed_run_with_a_multibyte_secret_remains_replayable() {
    let rig = Rig::new();
    let plan = rig.plan("test-credential", kill_steps());
    let resume_plan = plan.clone();
    let run_dir = rig.dir.path().join("runs").join(&plan.run_id);
    kill_mid_run(&rig, plan);
    assert_mid_kill(&run_dir);
    std::fs::write(run_dir.join("PROCEED"), "go\n").expect("PROCEED writes");
    let second = rig.engine().run(&resume_plan).await;
    check_multibyte_resume(&run_dir, &second);
}

/// §13: two daemons on one state file claim one item exactly once; a
/// later pass from both starts nothing (the open PR is observed).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_daemons_cannot_claim_the_same_item() {
    let daemons = Daemons::new();
    let (left, right) = daemons.pass_both().await;
    check_first_pass(&daemons, &left, &right);
    Daemons::join_runs(left, right).await;
    check_second_pass(&daemons).await;
}

/// Drives the plan on a private runtime until `block` is executing,
/// then drops the runtime mid-step: the SIGKILL simulation. Runs on its
/// own thread — a runtime cannot shut down inside an async context.
fn kill_mid_run(rig: &Rig, plan: RunPlan) {
    let engine = rig.engine();
    let run_dir = rig.dir.path().join("runs").join(&plan.run_id);
    std::thread::spawn(move || crash_at_step(engine, plan, &run_dir))
        .join()
        .expect("killer thread joins");
}

/// Phase 1 on the killer thread's runtime: spawn, wait, shut down.
/// Dropping the runtime drops the in-flight step future, whose child
/// dies through tokio's `kill_on_drop`.
fn crash_at_step(engine: Engine, plan: RunPlan, run_dir: &Path) {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime builds");
    let _task = runtime.spawn(async move { engine.run(&plan).await });
    runtime.block_on(wait_for_block(run_dir));
    runtime.shutdown_background();
}

/// Polls until `block`'s child has changed the worktree — the mid-step
/// kill point — panicking on a generous deadline.
async fn wait_for_block(run_dir: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !file_contains(&run_dir.join("wt/file.txt"), "changed") {
        assert!(Instant::now() < deadline, "block never ran");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Whether `path` holds `needle`; a missing file reads as empty.
fn file_contains(path: &Path, needle: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.contains(needle))
}

/// The log proves the kill landed mid-step: `first` finished, `block`
/// only started.
fn assert_mid_kill(run_dir: &Path) {
    let events = runlog::read_events(run_dir).expect("events read");
    assert_eq!(step_counts(&events, "first"), (1, 1));
    assert_eq!(step_counts(&events, "block"), (1, 0));
}

/// Resume skipped `first`, re-entered `block` once, and landed the PR.
fn check_resumed_run(run_dir: &Path, second: &RunOutcome) {
    let events = runlog::read_events(run_dir).expect("events read");
    let started = count_kind(&events, EventKind::RunStarted);
    let counts = (step_counts(&events, "first"), step_counts(&events, "block"));
    assert_eq!((started, counts), (1, ((1, 1), (2, 1))), "resume history");
    assert_eq!(second.outcome, StepOutcome::Success);
    check_seq(&events);
    check_pr(second);
    check_replay(run_dir);
}

/// The complete event stream resumes without re-running `first`.
fn check_multibyte_resume(run_dir: &Path, second: &RunOutcome) {
    assert_eq!(second.outcome, StepOutcome::Success);
    let events = runlog::read_events(run_dir).expect("events read");
    assert_eq!(step_counts(&events, "first"), (1, 1), "no re-run");
    check_replay(run_dir);
}

/// The log replays to the finished success — `state.json` is only a
/// cache of this.
fn check_replay(run_dir: &Path) {
    let state = runlog::replay_state(run_dir).expect("state replays");
    assert_eq!(state.status, RunStatus::Finished(StepOutcome::Success));
}

/// The `done` terminal opened the interrupted run's PR.
fn check_pr(second: &RunOutcome) {
    let pr = second.pr.as_ref().expect("a PR was opened");
    assert_eq!(pr.branch, format!("bureau/fix/{}", second.run_id));
    assert_eq!(pr.item_id.as_deref(), Some("42"));
}

/// How many events of `kind` a run recorded.
fn count_kind(events: &[Event], kind: EventKind) -> usize {
    events.iter().filter(|e| e.kind == kind).count()
}

/// (`step_started`, `step_finished`) counts for one step.
fn step_counts(events: &[Event], step: &str) -> (usize, usize) {
    let count = |kind| {
        events
            .iter()
            .filter(|e| e.kind == kind && step_is(e, step))
            .count()
    };
    (
        count(EventKind::StepStarted),
        count(EventKind::StepFinished),
    )
}

/// Whether the event's `step` payload field is `name`.
fn step_is(event: &Event, name: &str) -> bool {
    event.data.get("step").and_then(serde_json::Value::as_str) == Some(name)
}

/// Sequence numbers start at 0 and increase by one.
fn check_seq(events: &[Event]) {
    assert_eq!(events.first().map(|e| e.seq), Some(0));
    assert!(
        events.windows(2).all(|w| w[1].seq == w[0].seq + 1),
        "sequence numbers are contiguous"
    );
}

/// Exactly one run started across both daemons; exactly one lease held.
fn check_first_pass(
    daemons: &Daemons,
    left: &[bureau::reconcile::Started],
    right: &[bureau::reconcile::Started],
) {
    let started = left.len() + right.len();
    assert_eq!((started, daemons.live_leases()), (1, 1));
}

/// Both daemons observe the open PR: nothing starts, no lease is taken.
async fn check_second_pass(daemons: &Daemons) {
    let (left, right) = daemons.pass_both().await;
    let prs = daemons.observed_prs().await;
    assert_eq!(
        (left.len(), right.len(), prs, daemons.live_leases()),
        (0, 0, 1, 0)
    );
}
