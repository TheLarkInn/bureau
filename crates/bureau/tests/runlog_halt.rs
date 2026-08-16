//! Halted concurrent state remains durable across replay.

use bureau::adapters::{Execution, Usage};
use bureau::config::Completion;
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};
use bureau::runlog::{self, Event, EventKind};

#[test]
fn halted_member_and_group_survive_replay() {
    let blocked = Execution::new(result(), Usage::zero("fake")).halt();
    let state = runlog::replay(halted_events(&blocked)).expect("state");
    let group = &state.groups["inspect"];
    assert!(group.halted && group.members["blocked"].halted);
}

fn halted_events(blocked: &Execution) -> Vec<Event> {
    let mut events = started_events();
    append_halted(&mut events, blocked);
    events
}

fn started_events() -> Vec<Event> {
    let members = vec!["blocked".to_owned(), "cancelled".to_owned()];
    let mut events = Vec::new();
    push(
        &mut events,
        EventKind::RunStarted,
        runlog::run_started("run-1", "assignment"),
    );
    push(
        &mut events,
        EventKind::GroupStarted,
        runlog::group_started("inspect", &members, Completion::All, 2, "snapshot"),
    );
    events
}

fn append_halted(events: &mut Vec<Event>, blocked: &Execution) {
    push(
        events,
        EventKind::GroupMemberStarted,
        runlog::group_member_started("inspect", "blocked", 1),
    );
    push(
        events,
        EventKind::GroupMemberFinished,
        runlog::group_member_finished("inspect", "blocked", blocked),
    );
    push(
        events,
        EventKind::GroupMemberCancelled,
        runlog::group_member_cancelled("inspect", "cancelled", "group halted"),
    );
    push(
        events,
        EventKind::GroupFinished,
        runlog::group_finished("inspect", blocked),
    );
}

fn push(events: &mut Vec<Event>, kind: EventKind, data: serde_json::Value) {
    let seq = u64::try_from(events.len()).expect("sequence");
    events.push(Event {
        seq,
        at_ms: seq,
        kind,
        data,
    });
}

fn result() -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Blocked,
        outputs: std::collections::BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: "restore failed".to_owned(),
    }
}
