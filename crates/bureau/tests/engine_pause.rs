//! Pause-at-boundary: the run-directory PAUSE marker stops the machine
//! from starting the next step, exits the run resumable, and blocks
//! concurrent fan-out while it exists. Offline only.

#[path = "engine/rig.rs"]
mod rig;

use std::path::Path;

use bureau::config::{Completion, StepDef, StepKind};
use bureau::contract::StepOutcome;
use bureau::engine::RunOutcome;
use bureau::runlog::{self, Event, EventKind};

/// Reads one run's events, tolerating the still-open log's torn tail.
fn events(dir: &Path) -> Vec<Event> {
    runlog::read_events_tolerant(dir).expect("events read")
}

/// How many events of `kind` a run recorded.
fn count_kind(events: &[Event], kind: EventKind) -> usize {
    events.iter().filter(|event| event.kind == kind).count()
}

/// How many `GroupMemberStarted` events a run recorded.
fn member_starts(events: &[Event]) -> usize {
    count_kind(events, EventKind::GroupMemberStarted)
}

fn step_count(events: &[Event], step: &str) -> (usize, usize) {
    let is_step = |event: &Event| event.data["step"].as_str() == Some(step);
    let started = count_matching(events, EventKind::StepStarted, &is_step);
    let finished = count_matching(events, EventKind::StepFinished, &is_step);
    (started, finished)
}

fn count_matching(events: &[Event], kind: EventKind, is_step: &dyn Fn(&Event) -> bool) -> usize {
    events
        .iter()
        .filter(|event| event.kind == kind && is_step(event))
        .count()
}

fn pause_path(rig: &rig::Rig, run_id: &str) -> std::path::PathBuf {
    rig.dir.path().join("runs").join(run_id).join("PAUSE")
}

/// Touches every shared-rig helper this file's steps do not use.
fn use_helpers(rig: &rig::Rig) {
    let result = rig::result(StepOutcome::Success, "unused");
    let fixture = rig::fixture(rig.dir.path(), "unused.json", &result);
    let _ = (
        rig::agent_step("unused", &fixture, None),
        rig::decision_step("unused", "other"),
    );
}

fn linear_steps() -> Vec<StepDef> {
    vec![
        rig::det_step(
            "first",
            "touch ../PAUSE; echo changed >> file.txt",
            Some("second"),
        ),
        rig::det_step("second", "true", Some("done")),
    ]
}

/// A paused run never starts the step past the boundary, records no
/// terminal, and stays resumable.
fn check_paused(dir: &Path, outcome: &RunOutcome) {
    let events = events(dir);
    let seen = (
        step_count(&events, "first"),
        step_count(&events, "second"),
        count_kind(&events, EventKind::RunFinished),
        outcome.message.contains("paused"),
    );
    assert_eq!(seen, ((1, 1), (0, 0), 0, true), "events: {events:?}");
}

#[tokio::test]
async fn pause_marker_holds_the_run_at_the_step_boundary() {
    let rig = rig::Rig::new();
    use_helpers(&rig);
    let plan = rig.plan(linear_steps());
    let pause = pause_path(&rig, &plan.run_id);
    let first = rig.engine().run(&plan).await;
    check_paused(pause.parent().expect("run dir"), &first);
    std::fs::remove_file(&pause).expect("PAUSE removes");
    let second = rig.engine().run(&plan).await;
    assert_eq!(second.outcome, StepOutcome::Success);
    check_resumed(pause.parent().expect("run dir"));
}

/// Re-entry without the marker resumed past the boundary, untouched.
fn check_resumed(dir: &Path) {
    let events = events(dir);
    let seen = (
        step_count(&events, "first"),
        step_count(&events, "second"),
        count_kind(&events, EventKind::RunFinished),
    );
    assert_eq!(seen, ((1, 1), (1, 1), 1), "events: {events:?}");
}

/// A concurrent group paused before entry never starts, and removing
/// the marker lets the resumed run fan its members out.
#[tokio::test]
async fn pause_blocks_concurrent_fan_out_until_removed() {
    let rig = rig::Rig::new();
    use_helpers(&rig);
    let plan = rig.plan(group_steps());
    let pause = pause_path(&rig, &plan.run_id);
    let first = rig.engine().run(&plan).await;
    check_group_paused(pause.parent().expect("run dir"), &first);
    std::fs::remove_file(&pause).expect("PAUSE removes");
    let second = rig.engine().run(&plan).await;
    let dir = pause.parent().expect("run dir").to_path_buf();
    check_group_resumed(&dir, &second);
}

fn group_steps() -> Vec<StepDef> {
    let prepare = rig::det_step(
        "prepare",
        "touch ../PAUSE; echo changed >> file.txt",
        Some("inspect"),
    );
    let mut group = rig::step("inspect", StepKind::Concurrent);
    group.steps = vec!["test-a".to_owned(), "test-b".to_owned()];
    group.completion = Some(Completion::All);
    group.next = Some("done".to_owned());
    vec![
        prepare,
        group,
        rig::det_step("test-a", "printf alpha", None),
        rig::det_step("test-b", "printf beta", None),
    ]
}

/// A concurrent group paused before entry never starts: no group
/// snapshot, no member fan-out, no terminal.
fn check_group_paused(dir: &Path, outcome: &RunOutcome) {
    let events = events(dir);
    let seen = (
        count_kind(&events, EventKind::GroupStarted),
        member_starts(&events),
        count_kind(&events, EventKind::RunFinished),
        outcome.message.contains("paused"),
    );
    assert_eq!(seen, (0, 0, 0, true), "events: {events:?}");
}

/// After resume, the group enters once and fans out both members.
fn check_group_resumed(dir: &Path, outcome: &RunOutcome) {
    let events = events(dir);
    let seen = (
        count_kind(&events, EventKind::GroupStarted),
        member_starts(&events),
        outcome.outcome,
    );
    assert_eq!(seen, (1, 2, StepOutcome::Success), "events: {events:?}");
}
