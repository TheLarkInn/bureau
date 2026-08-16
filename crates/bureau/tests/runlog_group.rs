//! Durable concurrent-group run-log replay.

use std::path::PathBuf;

use bureau::adapters::{Execution, Usage};
use bureau::config::Completion;
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};
use bureau::runlog::{
    self, Event, EventKind, GroupRecord, RunState, group_finished, group_member_cancelled,
    group_member_finished, group_member_started, group_started, run_started,
};

const GROUP: &str = "inspect";
const SNAPSHOT: &str = "snapshot-sha";

fn result(outcome: StepOutcome, message: &str) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: std::collections::BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    }
}

fn execution(outcome: StepOutcome, message: &str, provider: &str) -> Execution {
    Execution::new(result(outcome, message), Usage::zero(provider))
}

fn push(events: &mut Vec<Event>, kind: EventKind, data: serde_json::Value) {
    let seq = u64::try_from(events.len()).expect("event count fits u64");
    events.push(Event {
        seq,
        at_ms: seq,
        kind,
        data,
    });
}

fn group_events(completion: Completion, names: &[&str]) -> Vec<Event> {
    let members: Vec<_> = names.iter().map(ToString::to_string).collect();
    let mut events = Vec::new();
    push(
        &mut events,
        EventKind::RunStarted,
        run_started("run-1", "assignment"),
    );
    push(
        &mut events,
        EventKind::GroupStarted,
        group_started(GROUP, &members, completion, 2, SNAPSHOT),
    );
    events
}

fn append_started(events: &mut Vec<Event>, member: &str) {
    push(
        events,
        EventKind::GroupMemberStarted,
        group_member_started(GROUP, member, 1),
    );
}

fn append_finished(events: &mut Vec<Event>, member: &str, outcome: StepOutcome) {
    append_started(events, member);
    let execution = execution(outcome, member, "fake");
    push(
        events,
        EventKind::GroupMemberFinished,
        group_member_finished(GROUP, member, &execution),
    );
}

fn append_cancelled(events: &mut Vec<Event>, member: &str, reason: &str) {
    push(
        events,
        EventKind::GroupMemberCancelled,
        group_member_cancelled(GROUP, member, reason),
    );
}

fn replay(events: Vec<Event>) -> RunState {
    runlog::replay(events).expect("run started")
}

fn partial_state() -> RunState {
    let names = ["finished", "running", "pending"];
    let mut events = group_events(Completion::All, &names);
    append_finished(&mut events, "finished", StepOutcome::Success);
    append_started(&mut events, "running");
    replay(events)
}

type MemberView<'a> = (
    &'a str,
    u32,
    Option<StepOutcome>,
    Option<&'a str>,
    Option<&'a str>,
);

fn member_views(group: &GroupRecord) -> Vec<MemberView<'_>> {
    group
        .members
        .iter()
        .map(|(name, member)| {
            (
                name.as_str(),
                member.attempts,
                member.result.as_ref().map(|result| result.outcome),
                member.usage.as_ref().map(|usage| usage.provider.as_str()),
                member.cancellation_reason.as_deref(),
            )
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Progress<'a> {
    completion: Completion,
    max_concurrent: usize,
    snapshot: &'a str,
    members: Vec<MemberView<'a>>,
    step: Option<(&'a str, Option<StepOutcome>)>,
}

fn progress(state: &RunState) -> Progress<'_> {
    let group = state.groups.get(GROUP).expect("group");
    let step = state
        .steps
        .last()
        .map(|record| (record.step.as_str(), record.outcome));
    Progress {
        completion: group.completion,
        max_concurrent: group.max_concurrent,
        snapshot: &group.snapshot,
        members: member_views(group),
        step,
    }
}

fn expected_partial_members() -> Vec<MemberView<'static>> {
    vec![
        (
            "finished",
            1,
            Some(StepOutcome::Success),
            Some("fake"),
            None,
        ),
        ("pending", 0, None, None, None),
        ("running", 1, None, None, None),
    ]
}

fn expected_partial() -> Progress<'static> {
    Progress {
        completion: Completion::All,
        max_concurrent: 2,
        snapshot: SNAPSHOT,
        members: expected_partial_members(),
        step: Some((GROUP, None)),
    }
}

#[test]
fn partial_group_replay_distinguishes_member_progress() {
    let state = partial_state();
    assert_eq!(progress(&state), expected_partial());
}

#[test]
fn cancellation_replay_keeps_reasons_without_fake_results() {
    let names = ["failed", "running", "pending"];
    let mut events = group_events(Completion::StopOnFailure, &names);
    append_finished(&mut events, "failed", StepOutcome::Failure);
    append_started(&mut events, "running");
    append_cancelled(&mut events, "running", "failure stopped group");
    append_cancelled(&mut events, "pending", "failure stopped group");
    let state = replay(events);
    let group = state.groups.get(GROUP).expect("group");
    assert_eq!(
        member_views(group),
        vec![
            ("failed", 1, Some(StepOutcome::Failure), Some("fake"), None),
            ("pending", 0, None, None, Some("failure stopped group")),
            ("running", 1, None, None, Some("failure stopped group")),
        ]
    );
}

fn aggregate_execution() -> Execution {
    let mut aggregate = execution(StepOutcome::Success, "aggregate", "concurrent");
    aggregate
        .result
        .outputs
        .insert("member_count".to_owned(), serde_json::json!(2));
    aggregate
}

fn finished_state(aggregate: &Execution) -> RunState {
    let mut events = group_events(Completion::All, &["first", "second"]);
    append_finished(&mut events, "first", StepOutcome::Success);
    append_finished(&mut events, "second", StepOutcome::NoWork);
    push(
        &mut events,
        EventKind::GroupFinished,
        group_finished(GROUP, aggregate),
    );
    replay(events)
}

#[derive(Debug, PartialEq)]
struct AggregateView<'a> {
    outcome: Option<StepOutcome>,
    step_result: Option<&'a StepResult>,
    step_usage: Option<&'a Usage>,
    group_result: Option<&'a StepResult>,
    group_usage: Option<&'a Usage>,
}

fn aggregate_view(state: &RunState) -> AggregateView<'_> {
    let group = state.groups.get(GROUP).expect("group");
    let step = state.steps.last().expect("normal step");
    AggregateView {
        outcome: step.outcome,
        step_result: step.result.as_ref(),
        step_usage: step.usage.as_ref(),
        group_result: group.result.as_ref(),
        group_usage: group.usage.as_ref(),
    }
}

const fn expected_aggregate(execution: &Execution) -> AggregateView<'_> {
    AggregateView {
        outcome: Some(StepOutcome::Success),
        step_result: Some(&execution.result),
        step_usage: Some(&execution.usage),
        group_result: Some(&execution.result),
        group_usage: Some(&execution.usage),
    }
}

#[test]
fn group_finished_completes_the_normal_step_with_full_aggregate() {
    let aggregate = aggregate_execution();
    let state = finished_state(&aggregate);
    assert_eq!(aggregate_view(&state), expected_aggregate(&aggregate));
}

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("runlog-group-cache-{}", std::process::id()))
}

#[test]
fn state_cache_round_trip_preserves_partial_groups() {
    let state = partial_state();
    let dir = cache_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cache directory");
    runlog::write_state_cache(&dir, &state).expect("write cache");
    let bytes = std::fs::read(dir.join(runlog::STATE_FILE)).expect("read cache");
    let cached: RunState = serde_json::from_slice(&bytes).expect("decode cache");
    std::fs::remove_dir_all(dir).expect("remove cache directory");
    assert_eq!(cached, state);
}

#[test]
fn out_of_order_and_unknown_group_events_are_ignored() {
    let names = ["first", "second"];
    let expected = replay(group_events(Completion::All, &names));
    let mut events = group_events(Completion::All, &names);
    push(
        &mut events,
        EventKind::StepStarted,
        runlog::step_started("other"),
    );
    push(
        &mut events,
        EventKind::StepFinished,
        runlog::step_finished("other", StepOutcome::Success),
    );
    let aggregate = aggregate_execution();
    push(
        &mut events,
        EventKind::GroupFinished,
        group_finished(GROUP, &aggregate),
    );
    assert_eq!(replay(events), expected);
}
