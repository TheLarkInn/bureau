//! Concurrent evidence execution, isolation, routing, and cancellation.

#[path = "engine/rig.rs"]
mod rig;

use std::time::{Duration, Instant};

use bureau::config::{Completion, StepDef, StepKind};
use bureau::contract::StepOutcome;
use bureau::runlog::{self, EventKind};

#[tokio::test]
async fn concurrent_members_feed_one_namespaced_result() {
    let rig = rig::Rig::new();
    use_helpers(&rig);
    let plan = rig.plan(success_steps(None));
    let run_id = plan.run_id.clone();
    let outcome = rig.engine().run(&plan).await;
    let events = events(&rig, &run_id);
    let first_finish = position(&events, EventKind::GroupMemberFinished);
    let starts_before = events[..first_finish]
        .iter()
        .filter(|event| event.kind == EventKind::GroupMemberStarted)
        .count();
    assert_eq!((outcome.outcome, starts_before), (StepOutcome::Success, 2));
}

#[tokio::test]
async fn group_limit_one_serializes_member_starts() {
    let rig = rig::Rig::new();
    let plan = rig.plan(success_steps(Some(1)));
    let run_id = plan.run_id.clone();
    let outcome = rig.engine().run(&plan).await;
    let kinds = member_kinds(events(&rig, &run_id));
    let expected = vec![
        EventKind::GroupMemberStarted,
        EventKind::GroupMemberFinished,
        EventKind::GroupMemberStarted,
        EventKind::GroupMemberFinished,
    ];
    assert_eq!((outcome.outcome, kinds), (StepOutcome::Success, expected));
}

fn member_kinds(events: Vec<runlog::Event>) -> Vec<EventKind> {
    events
        .into_iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::GroupMemberStarted | EventKind::GroupMemberFinished
            )
        })
        .map(|event| event.kind)
        .collect()
}

#[tokio::test]
async fn stop_on_failure_cancels_a_slow_sibling() {
    let rig = rig::Rig::new();
    let plan = rig.plan(failing_steps());
    let run_id = plan.run_id.clone();
    let started = Instant::now();
    let outcome = rig.engine().run(&plan).await;
    let events = events(&rig, &run_id);
    let cancelled = events
        .iter()
        .any(|event| event.kind == EventKind::GroupMemberCancelled);
    assert_eq!(
        (
            outcome.outcome,
            cancelled,
            started.elapsed() < Duration::from_secs(5),
        ),
        (StepOutcome::Failure, true, true)
    );
}

fn success_steps(limit: Option<u32>) -> Vec<StepDef> {
    let prepare = rig::det_step("prepare", "echo changed >> file.txt", Some("inspect"));
    let group = group(
        &["test-a", "test-b"],
        Completion::All,
        limit,
        Some("evaluate"),
    );
    let first = rig::det_step(
        "test-a",
        "echo leak > evidence-a; sleep 0.2; printf alpha",
        None,
    );
    let second = rig::det_step(
        "test-b",
        "echo leak > evidence-b; sleep 0.2; printf beta",
        None,
    );
    let mut evaluate = rig::det_step(
        "evaluate",
        "cat > group.json; grep -q '\"inspect\"' group.json; grep -q test-a group.json; grep -q test-b group.json; test ! -e evidence-a; test ! -e evidence-b",
        Some("done"),
    );
    evaluate.inputs_from = vec!["inspect".to_owned()];
    vec![prepare, group, first, second, evaluate]
}

fn failing_steps() -> Vec<StepDef> {
    let mut group = group(
        &["fail-fast", "slow"],
        Completion::StopOnFailure,
        None,
        Some("done"),
    );
    group.on_failure = Some("abort".to_owned());
    vec![
        group,
        rig::det_step("fail-fast", "sleep 0.1; exit 1", None),
        rig::det_step("slow", "sleep 30", None),
    ]
}

fn group(
    members: &[&str],
    completion: Completion,
    limit: Option<u32>,
    next: Option<&str>,
) -> StepDef {
    let mut group = rig::step("inspect", StepKind::Concurrent);
    group.steps = members.iter().map(|member| (*member).to_owned()).collect();
    group.completion = Some(completion);
    group.max_concurrent = limit;
    group.next = next.map(str::to_owned);
    group
}

fn events(rig: &rig::Rig, run_id: &str) -> Vec<runlog::Event> {
    let directory = rig.dir.path().join("runs").join(run_id);
    runlog::read_events(&directory).expect("events")
}

fn position(events: &[runlog::Event], kind: EventKind) -> usize {
    events
        .iter()
        .position(|event| event.kind == kind)
        .expect("event kind")
}

fn use_helpers(rig: &rig::Rig) {
    let result = rig::result(StepOutcome::Success, "unused");
    let fixture = rig::fixture(rig.dir.path(), "unused.json", &result);
    let _ = (
        rig::agent_step("unused", &fixture, None),
        rig::decision_step("unused", "other"),
    );
}
