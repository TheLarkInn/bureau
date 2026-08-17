//! Assertions over a finished run: its event log, the forge's records,
//! and the bare remote's refs.

use bureau::contract::StepOutcome;
use bureau::engine::RunOutcome;
use bureau::forge::{Forge as _, Pr};
use bureau::runlog::{self, Event, EventKind, RunStatus};

use super::rig::Rig;

/// Reads one run's events.
fn events(rig: &Rig, run_id: &str) -> Vec<Event> {
    let dir = rig.dir.path().join("runs").join(run_id);
    runlog::read_events(&dir).expect("events read")
}

/// Whether the event's `step` payload field is `name`.
fn step_is(event: &Event, name: &str) -> bool {
    event.data.get("step").and_then(serde_json::Value::as_str) == Some(name)
}

/// Sequence numbers start at 0 and increase by exactly one.
fn check_seq(events: &[Event]) {
    assert_eq!(events.first().map(|e| e.seq), Some(0));
    assert!(
        events.windows(2).all(|w| w[1].seq == w[0].seq + 1),
        "sequence numbers are contiguous"
    );
}

/// No step finished more often than its attempt budget allows.
fn check_attempts(events: &[Event], budget: &[(&str, usize)]) {
    for (name, max) in budget {
        let finished = events
            .iter()
            .filter(|e| e.kind == EventKind::StepFinished && step_is(e, name))
            .count();
        assert!(
            finished <= *max,
            "{name} finished {finished} times, budget {max}"
        );
    }
}

/// The run's events are sound: contiguous seqs, and every step within
/// its attempt budget (propose 3, everything else 1).
pub fn events_in_order(rig: &Rig, run_id: &str) {
    let events = events(rig, run_id);
    check_seq(&events);
    check_attempts(&events, &[("propose", 3), ("apply", 1), ("review", 1)]);
}

/// The event log replays to `Finished(expected)` with all six code
/// steps recorded (decisions run no code and leave no events).
pub fn replay(rig: &Rig, run_id: &str, expected: StepOutcome, steps: usize) {
    let dir = rig.dir.path().join("runs").join(run_id);
    let state = runlog::replay_state(&dir).expect("state replays");
    assert_eq!(state.status, RunStatus::Finished(expected));
    assert_eq!(state.steps.len(), steps, "recorded steps");
}

/// The forge's one PR: the branch carries the assignment's prefix and
/// the PR links back to the item.
pub fn single_pr(outcome: &RunOutcome) -> Pr {
    let pr = outcome.pr.clone().expect("a PR was opened");
    assert!(pr.branch.starts_with("bureau/fix/"), "branch {}", pr.branch);
    assert_eq!(pr.item_id.as_deref(), Some("42"));
    pr
}

/// The forge holds exactly `expected` PRs under the assignment prefix.
pub async fn pr_count(rig: &Rig, expected: usize) {
    let prs = rig
        .forge
        .open_prs(&rig.url, "bureau/fix/")
        .await
        .expect("open_prs");
    assert_eq!(prs.len(), expected, "PRs on the fake forge");
}

/// The bare remote holds the pushed run branch.
pub fn remote_branch(rig: &Rig, branch: &str) {
    let output = std::process::Command::new("git")
        .args(["--git-dir", &rig.url, "rev-parse", "--verify", branch])
        .output()
        .expect("git runs");
    assert!(output.status.success(), "remote lacks {branch}");
}

/// Exactly one escalation comment landed on the item, naming `step`.
pub fn escalated(rig: &Rig, step: &str) {
    let comments = rig.forge.comments();
    assert_eq!(comments.len(), 1, "one escalation comment");
    let (item, body) = &comments[0];
    assert!(
        item == "42" && body.contains(step),
        "comment names {step}: {body}"
    );
}
