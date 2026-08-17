//! The walking-skeleton behavior port (goober's test/e2e/
//! `walking_skeleton_test.go`): one work item through the real engine —
//! real event log on disk, real git worktrees, the `fake` adapter
//! standing in for the agent CLI, no network. Asserts on the log, not
//! on engine internals: event sequence, terminal state, replay.

#[path = "behavior/skeleton_steps.rs"]
mod steps;
#[path = "behavior/skeleton_support.rs"]
mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use bureau::contract::StepOutcome;
use bureau::engine::{Engine, RunOutcome, RunPlan};
use bureau::runlog::{self, EventKind, RunStatus};
use support::{Rig, check_seq, normalized, step_counts};
use tokio::runtime::Builder;

/// The blocking step's work: change the worktree, then wait out the
/// kill (or the PROCEED marker). Bounded, so a leaked child dies.
const BLOCK_RUN: &str = "echo change >> impl.txt; i=0; while [ $i -lt 1200 ] && [ ! -f ../PROCEED ]; do sleep 0.05; i=$((i+1)); done";

/// The repass scenario's two agent fixtures.
fn fixtures(rig: &Rig) -> (String, String) {
    let ok = support::result(StepOutcome::Success, "done");
    (
        support::fixture(rig.dir.path(), "implement.json", &ok),
        support::fixture(rig.dir.path(), "review.json", &ok),
    )
}

/// Every code step of the repass pipeline ran its expected attempts:
/// the repass doubled implement/apply/review/policy; local-ci ran once.
fn check_repass_counts(events: &[runlog::Event]) {
    let counts = [
        step_counts(events, "implement"),
        step_counts(events, "apply"),
        step_counts(events, "review"),
        step_counts(events, "policy"),
        step_counts(events, "local-ci"),
    ];
    assert_eq!(counts, [(2, 2), (2, 2), (2, 2), (2, 2), (1, 1)]);
}

/// The log brackets the run and replays to the finished success.
fn check_log(rig: &Rig, run_id: &str) {
    let events = rig.events(run_id);
    let frame = (
        events.first().map(|e| e.kind),
        events.last().map(|e| e.kind),
    );
    assert_eq!(
        frame,
        (Some(EventKind::RunStarted), Some(EventKind::RunFinished))
    );
    check_seq(&events);
    let dir = rig.dir.path().join("runs").join(run_id);
    let state = runlog::replay_state(&dir).expect("state replays");
    assert_eq!(state.status, RunStatus::Finished(StepOutcome::Success));
}

/// The headline skeleton (goober issue #29's port): the verdict rejects
/// the first pass, routes it back to implement, and the repass passes;
/// local-ci runs and the run completes with its PR.
#[tokio::test]
async fn the_skeleton_completes_after_one_review_repass() {
    let rig = Rig::new();
    let (implement, review) = fixtures(&rig);
    let plan = rig.plan(steps::repass_steps(&implement, &review));
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::Success);
    let events = rig.events(&outcome.run_id);
    check_repass_counts(&events);
    check_log(&rig, &outcome.run_id);
    let pr = outcome.pr.expect("a PR was opened");
    assert!(
        pr.branch.starts_with("bureau/fix/") && pr.item_id.as_deref() == Some("101"),
        "branch {} links item 101",
        pr.branch
    );
}

/// The rejecting review (goober's gate-fail port): the verdict routes
/// failure to `abort` and the run ends in failure without a repass.
#[tokio::test]
async fn a_rejecting_review_aborts_the_run() {
    let rig = Rig::new();
    let (implement, review) = fixtures(&rig);
    let plan = rig.plan(steps::abort_steps(&implement, &review));
    let outcome = rig.engine().run(&plan).await;
    let events = rig.events(&outcome.run_id);
    let state = (
        outcome.outcome,
        outcome.pr.is_none(),
        step_counts(&events, "implement"),
        step_counts(&events, "local-ci"),
    );
    assert_eq!(state, (StepOutcome::Failure, true, (1, 1), (0, 0)));
    let dir = rig.dir.path().join("runs").join(&outcome.run_id);
    let replayed = runlog::replay_state(&dir).expect("state replays");
    assert_eq!(replayed.status, RunStatus::Finished(StepOutcome::Failure));
}

/// The conformance seed's port: two independent runs of one plan, each
/// with one genuine policy retry (`apply` fails its first attempt and
/// is retried within its budget), log identical normalized event
/// sequences — the fake adapter removes all live-agent variance.
#[tokio::test]
async fn two_runs_of_one_plan_log_identical_event_sequences() {
    let (a, b) = (run_once().await, run_once().await);
    assert_eq!(a, b, "identical inputs must log identical sequences");
}

/// One canonical run of the repass pipeline with a retrying `apply`.
async fn run_once() -> Vec<(EventKind, Option<String>, Option<String>)> {
    let rig = Rig::new();
    let (implement, review) = fixtures(&rig);
    let mut steps = steps::repass_steps(&implement, &review);
    steps[1].run = Some(RETRY_ONCE.to_owned());
    // A failed attempt re-enters the step through its own failure edge —
    // the task-level policy retry — and the repass re-enters it once more.
    steps[1].on_failure = Some("apply".to_owned());
    steps[1].max_attempts = 3;
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    assert_eq!(outcome.outcome, StepOutcome::Success, "{}", outcome.message);
    let events = rig.events(&outcome.run_id);
    check_seq(&events);
    assert_eq!(step_counts(&events, "apply"), (3, 3), "policy retry ran");
    normalized(&events)
}

/// A first attempt that fails once per run directory — the retried
/// attempt the conformance comparison must include.
const RETRY_ONCE: &str =
    "echo change >> impl.txt; [ -f ../.retried ] || { touch ../.retried; exit 1; }";

/// The crash/resume acceptance port: kill the engine mid-`apply`,
/// restart against a fresh `Engine`, and resume from the event log —
/// the interrupted attempt is re-entered within its budget and the run
/// rejoins the skeleton machine (review, verdict, local-ci) to a PR.
#[tokio::test]
async fn a_crash_mid_apply_resumes_and_completes() {
    let rig = Rig::new();
    let (implement, review) = fixtures(&rig);
    let plan = rig.plan(crash_steps(&implement, &review));
    let resume = plan.clone();
    let run_dir = rig.dir.path().join("runs").join(&plan.run_id);
    kill_mid_run(&rig, plan);
    assert_mid_kill(&rig.events(&resume.run_id));
    std::fs::write(run_dir.join("PROCEED"), "go\n").expect("PROCEED writes");
    let second = rig.engine().run(&resume).await;
    check_resumed(&rig, &second);
}

/// The crash variant: `apply` blocks until killed or PROCEED appears,
/// with a second attempt budgeted for the resume's re-entry.
fn crash_steps(implement: &str, review: &str) -> Vec<bureau::config::StepDef> {
    let mut apply = steps::det_step("apply", BLOCK_RUN, "review", None);
    apply.max_attempts = 2;
    let verdict = steps::decision_step("verdict", "review", "local-ci", "implement");
    vec![
        steps::agent_step("implement", "implementer", implement, "apply"),
        apply,
        steps::agent_step("review", "reviewer", review, "verdict"),
        verdict,
        steps::det_step("local-ci", "test -f impl.txt", "done", None),
    ]
}

/// Drives the plan on a private runtime until `apply` has changed the
/// worktree, then drops the runtime mid-step: the SIGKILL simulation.
fn kill_mid_run(rig: &Rig, plan: RunPlan) {
    let engine: Engine = rig.engine();
    let run_dir = rig.dir.path().join("runs").join(&plan.run_id);
    std::thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime builds");
        let _task = runtime.spawn(async move { engine.run(&plan).await });
        runtime.block_on(wait_for_change(&run_dir));
        runtime.shutdown_background();
    })
    .join()
    .expect("killer thread joins");
}

/// Polls until `apply`'s child has changed the worktree — the mid-step
/// kill point — panicking on a generous deadline.
async fn wait_for_change(run_dir: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !file_contains(&run_dir.join("wt/impl.txt"), "change") {
        assert!(Instant::now() < deadline, "apply never ran");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Whether `path` holds `needle`; a missing file reads as empty.
fn file_contains(path: &Path, needle: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.contains(needle))
}

/// The log proves the kill landed mid-step: `apply` only started.
fn assert_mid_kill(events: &[runlog::Event]) {
    let counts = (
        step_counts(events, "implement"),
        step_counts(events, "apply"),
    );
    assert_eq!(counts, ((1, 1), (1, 0)), "killed mid-apply");
}

/// Resume re-entered `apply` once, ran review and local-ci once each,
/// and landed the PR — the same machine the crash-free runs exercise.
fn check_resumed(rig: &Rig, second: &RunOutcome) {
    let events = rig.events(&second.run_id);
    let started = events
        .iter()
        .filter(|e| e.kind == EventKind::RunStarted)
        .count();
    let counts = [
        step_counts(&events, "apply"),
        step_counts(&events, "review"),
        step_counts(&events, "local-ci"),
    ];
    assert_eq!((started, counts), (1, [(2, 1), (1, 1), (1, 1)]));
    assert_eq!(second.outcome, StepOutcome::Success);
    assert!(second.pr.is_some(), "the resumed run landed its PR");
}
