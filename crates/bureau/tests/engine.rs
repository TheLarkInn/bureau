//! Layer 4 engine tests (DESIGN.md sections 7 and 11): the state
//! machine, its terminals, resume, and secret scrubbing — all offline
//! against local git repos and the `fake` adapter and forge.

#[path = "engine/rig.rs"]
mod rig;

use std::path::Path;

use bureau::contract::{StepOutcome, Trust};
use bureau::engine::{RunOutcome, RunPlan};
use bureau::forge::Forge as _;
use bureau::process::{REDACTED, Secret};
use bureau::runlog::{self, Event, EventKind};
use rig::{Rig, agent_step, decision_step, det_step, fixture, result};

/// Reads one run's events.
fn events(rig: &Rig, run_id: &str) -> Vec<Event> {
    let dir = rig.dir.path().join("runs").join(run_id);
    runlog::read_events(&dir).expect("events read")
}

/// How many events of `kind` a run recorded.
fn count_kind(events: &[Event], kind: EventKind) -> usize {
    events.iter().filter(|e| e.kind == kind).count()
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

/// The linear run logged its lifecycle in order, with streamed output.
fn check_linear_events(rig: &Rig, run_id: &str) {
    let events = events(rig, run_id);
    let wanted = [
        EventKind::RunStarted,
        EventKind::StepStarted,
        EventKind::Checkpoint,
        EventKind::StepFinished,
        EventKind::StepStarted,
        EventKind::Checkpoint,
        EventKind::StepFinished,
        EventKind::BranchPushed,
        EventKind::PrCreated,
        EventKind::RunFinished,
    ];
    let found: Vec<EventKind> = events
        .iter()
        .map(|e| e.kind)
        .filter(|k| *k != EventKind::Output)
        .collect();
    assert_eq!(found, wanted);
    assert!(
        events.iter().any(|e| e.kind == EventKind::Output),
        "streamed step output missing"
    );
    check_seq(&events);
}

/// The done terminal opened the run's PR on the fake forge.
fn check_pr(rig: &Rig, outcome: &RunOutcome) {
    let pr = outcome.pr.as_ref().expect("a PR was opened");
    assert_eq!(pr.branch, format!("bureau/fix/{}", outcome.run_id));
    assert_eq!(pr.item_id.as_deref(), Some("42"));
    assert_eq!(rig.forge.comments().len(), 0, "no escalation happened");
}

/// The escalation left exactly one comment naming `reason`.
fn check_escalation(rig: &Rig, reason: &str) {
    let comments = rig.forge.comments();
    assert_eq!(comments.len(), 1, "one escalation comment");
    assert!(
        comments[0].1.contains(reason),
        "comment names the reason: {}",
        comments[0].1
    );
}

/// How often a step finished (its attempts stop at `max_attempts`).
fn check_step_attempts(rig: &Rig, run_id: &str, step: &str, expected: usize) {
    let events = events(rig, run_id);
    let finished = events
        .iter()
        .filter(|e| e.kind == EventKind::StepFinished && step_is(e, step))
        .count();
    assert_eq!(finished, expected, "attempts stopped at max");
}

/// The pipeline used by the success and resume tests.
fn linear_steps(rig: &Rig) -> Vec<bureau::config::StepDef> {
    let transcript = fixture(
        rig.dir.path(),
        "propose.json",
        &result(StepOutcome::Success, "patched"),
    );
    vec![
        det_step("edit", "echo changed >> file.txt", Some("propose")),
        agent_step("propose", &transcript, Some("done")),
    ]
}

#[tokio::test]
async fn linear_pipeline_succeeds_and_opens_a_pr() {
    let rig = Rig::new();
    let outcome = rig.engine().run(&rig.plan(linear_steps(&rig))).await;
    assert_eq!(outcome.outcome, StepOutcome::Success);
    check_pr(&rig, &outcome);
    check_linear_events(&rig, &outcome.run_id);
}

#[tokio::test]
async fn a_run_with_no_changes_is_no_work() {
    let rig = Rig::new();
    let steps = vec![det_step("check", "true", Some("done"))];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    assert_eq!(outcome.outcome, StepOutcome::NoWork);
    assert!(outcome.pr.is_none(), "no PR without changes");
    check_no_prs(&rig).await;
}

/// The fake forge recorded no PRs.
async fn check_no_prs(rig: &Rig) {
    let prs = rig
        .forge
        .open_prs("main", "bureau/")
        .await
        .expect("open_prs");
    assert!(prs.is_empty(), "NoWork opened no PR");
}

#[tokio::test]
async fn a_retry_loop_escalates_at_max_attempts() {
    let rig = Rig::new();
    let failing = fixture(
        rig.dir.path(),
        "propose.json",
        &result(StepOutcome::Failure, "cannot fix"),
    );
    let outcome = rig.engine().run(&rig.plan(retry_steps(&failing))).await;
    assert_eq!(outcome.outcome, StepOutcome::Blocked);
    check_escalation(&rig, "exceeded max attempts");
    check_step_attempts(&rig, &outcome.run_id, "propose", 2);
}

/// propose (fails) → decide → propose, twice, then escalate.
fn retry_steps(failing: &str) -> Vec<bureau::config::StepDef> {
    let mut propose = agent_step("propose", failing, Some("done"));
    propose.on_failure = Some("decide".to_owned());
    propose.max_attempts = 2;
    vec![propose, decision_step("decide", "propose")]
}

#[tokio::test]
async fn a_missing_edge_aborts_the_run() {
    let rig = Rig::new();
    let steps = vec![det_step("edit", "true", None)];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    assert_eq!(outcome.outcome, StepOutcome::Failure);
    assert!(
        outcome.message.contains("no edge"),
        "message: {}",
        outcome.message
    );
}

#[tokio::test]
async fn a_step_above_its_input_trust_is_blocked() {
    let rig = Rig::new();
    let transcript = fixture(
        rig.dir.path(),
        "trusted.json",
        &result(StepOutcome::Success, "ok"),
    );
    let mut step = agent_step("propose", &transcript, Some("done"));
    step.trust = Some(Trust::Trusted);
    let outcome = rig.engine().run(&rig.plan(vec![step])).await;
    assert_eq!(outcome.outcome, StepOutcome::Blocked);
    check_trust_gate(&rig, &outcome);
}

/// The gated step never started and the comment says why.
fn check_trust_gate(rig: &Rig, outcome: &RunOutcome) {
    let events = events(rig, &outcome.run_id);
    assert_eq!(
        count_kind(&events, EventKind::StepStarted),
        0,
        "no spawn happened"
    );
    check_escalation(rig, "trust");
}

#[tokio::test]
async fn a_cancel_marker_stops_the_run_between_steps() {
    let rig = Rig::new();
    let steps = vec![
        det_step("mark", "touch ../CANCEL", Some("second")),
        det_step("second", "true", Some("done")),
    ];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    assert_eq!(outcome.outcome, StepOutcome::Failure);
    assert_eq!(outcome.message, "cancelled");
}

#[tokio::test]
async fn a_finished_run_resumes_to_its_recorded_outcome() {
    let rig = Rig::new();
    let plan = rig.plan(linear_steps(&rig));
    let first = rig.engine().run(&plan).await;
    assert_eq!(first.outcome, StepOutcome::Success);
    check_resume(&rig, &plan).await;
}

/// A second run of the same id appends nothing and returns Success.
async fn check_resume(rig: &Rig, plan: &RunPlan) {
    let before = events(rig, &plan.run_id).len();
    let second = rig.engine().run(plan).await;
    assert_eq!(second.outcome, StepOutcome::Success);
    assert_eq!(
        events(rig, &plan.run_id).len(),
        before,
        "resume appended nothing"
    );
}

#[tokio::test]
async fn credentials_never_reach_the_event_log() {
    let rig = Rig::new();
    let run = r#"echo "env=${BUREAU_CREDENTIAL_API_TOKEN:+present}" && echo token=hunter7secret"#;
    let steps = vec![det_step("echo", run, Some("done"))];
    let mut plan = rig.plan(steps);
    plan.credentials
        .insert("api-token".to_owned(), Secret::new("hunter7secret"));
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::NoWork);
    check_scrubbed(rig.dir.path(), &plan.run_id);
}

/// The raw log holds the redaction marker, never the secret — and the
/// step's environment never held the credential at all (§10).
fn check_scrubbed(dir: &Path, run_id: &str) {
    let path = dir.join("runs").join(run_id).join(runlog::EVENTS_FILE);
    let raw = std::fs::read_to_string(path).expect("events file");
    assert!(
        !raw.contains("hunter7secret") && raw.contains(REDACTED),
        "the echoed secret was scrubbed, never leaked"
    );
    assert!(
        !raw.contains("env=present"),
        "BUREAU_CREDENTIAL_API_TOKEN reached the step env"
    );
}
