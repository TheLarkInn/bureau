use bureau::runlog::copilot_factory::{Operation, Record, Records, RuntimePurpose};
use bureau::runlog::{Event, EventKind};
use serde_json::{Value, json};

use super::fixture::Fixture;

fn first(events: &[Event], tag: &str) -> Event {
    events
        .iter()
        .find(|event| event.data["event"] == tag)
        .expect("native lifecycle fact")
        .clone()
}

fn changed(events: &[Event], tag: &str, pointer: &str, value: Value) -> Vec<Event> {
    let mut events = events.to_vec();
    let event = events
        .iter_mut()
        .find(|event| event.data["event"] == tag)
        .expect("native fact");
    *event
        .data
        .pointer_mut(pointer)
        .expect("recorded native field") = value;
    events
}

fn native(events: &[Event]) -> Vec<Event> {
    events
        .iter()
        .filter(|event| event.kind == EventKind::CopilotFactory)
        .cloned()
        .collect()
}

fn rejected_cases(events: &[Event]) {
    for (tag, pointer, value) in [
        ("accepted", "/run_id", json!("")),
        ("accepted", "/attempt", json!(2)),
        ("observed", "/run/runId", json!("different-root")),
        ("observed", "/run/attempt", json!(2)),
        ("observed", "/summary/canResume", Value::Null),
        ("observed", "/summary/consumed/nanoAiu", json!(-1)),
        ("runtime_closed", "/purpose", json!("inspection")),
        ("prepared", "/intent/step_attempt", json!(0)),
        ("prepared", "/intent/request/schema", json!("v1")),
    ] {
        assert!(
            Records::replay(&changed(events, tag, pointer, value)).is_err(),
            "{tag}: {pointer}"
        );
    }
}

fn missing_cases(events: &[Event]) {
    for tag in [
        "prepared",
        "runtime_opened",
        "session_accepted",
        "dispatch",
        "accepted",
    ] {
        let remaining: Vec<_> = events
            .iter()
            .filter(|event| event.data["event"] != tag)
            .cloned()
            .collect();
        assert!(Records::replay(&remaining).is_err(), "missing {tag}");
    }
}

fn interrupted_bootstrap() -> [fn(&mut Record); 13] {
    [
        |record| record.session_accepted = false,
        |record| record.execution_clean = false,
        |record| record.active_runtime = Some(RuntimePurpose::Execution),
        |record| record.active_runtime = Some(RuntimePurpose::Inspection),
        |record| record.pending_admission = Some(Operation::Start),
        |record| record.pending_admission = Some(Operation::Resume),
        |record| record.dispatched = Some(Operation::Start),
        |record| record.dispatched = Some(Operation::Resume),
        |record| record.dispatched = Some(Operation::Pause),
        |record| record.dispatched = Some(Operation::Cancel),
        |record| record.run_id = Some("accepted-run".into()),
        |record| record.rejected = Some("rejected".into()),
        |record| record.indeterminate = Some("unknown admission".into()),
    ]
}

#[tokio::test]
async fn bootstrap_start_eligibility_requires_every_checked_native_boundary() {
    let fixture = Fixture::create("bootstrap-pause");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let original = fixture.record();
    assert_eq!(
        (original.can_start(), original.accounting_complete()),
        (true, false)
    );
    for changed in interrupted_bootstrap() {
        let mut record = original.clone();
        changed(&mut record);
        assert!(!record.can_start());
    }
}

#[tokio::test]
async fn dispatched_native_runs_never_become_bootstrap_candidates() {
    for mode in ["success", "pause", "ambiguous"] {
        let fixture = Fixture::create(mode);
        let _outcome = fixture.engine.run(&fixture.plan).await;
        let record = fixture.record();
        assert_eq!(
            (
                record.dispatched.is_some(),
                record.can_start(),
                record.ambiguous_start()
            ),
            (true, false, mode == "ambiguous"),
            "{mode}"
        );
    }
}

#[tokio::test]
async fn actual_engine_facts_refuse_missing_or_changed_native_admission_and_authority() {
    let fixture = Fixture::create("success");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let events = native(&fixture.events());
    rejected_cases(&events);
    missing_cases(&events);
}

#[tokio::test]
async fn duplicate_intents_or_another_native_session_for_the_same_step_are_invalid() {
    let fixture = Fixture::create("success");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let events = native(&fixture.events());
    for session in [fixture.record().intent.session_id, "another-session".into()] {
        let mut duplicate = first(&events, "prepared");
        duplicate.data["intent"]["session_id"] = json!(session);
        let mut altered = events.clone();
        altered.push(duplicate);
        assert!(Records::replay(&altered).is_err());
    }
}

#[tokio::test]
async fn cumulative_native_accounting_can_never_roll_back() {
    let fixture = Fixture::create("success");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let events = native(&fixture.events());
    let close = events
        .iter()
        .position(|event| event.data["event"] == "runtime_closed")
        .expect("shutdown");
    for counter in ["activeMs", "subagents", "nanoAiu"] {
        let mut later = first(&events, "observed");
        later.data["summary"]["consumed"][counter] = json!(0);
        let mut altered = events.clone();
        altered.insert(close, later);
        assert!(Records::replay(&altered).is_err(), "{counter}");
    }
}

fn inspection(events: &[Event]) -> Vec<Event> {
    let mut opened = first(events, "runtime_opened");
    opened.data["purpose"] = json!("inspection");
    let mut closed = first(events, "runtime_closed");
    closed.data["purpose"] = json!("inspection");
    vec![
        opened,
        first(events, "session_accepted"),
        first(events, "observed"),
        closed,
    ]
}

#[tokio::test]
async fn a_clean_inspection_cannot_replace_a_missing_execution_shutdown() {
    let fixture = Fixture::create("pause");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let events = native(&fixture.events());
    let mut altered: Vec<_> = events
        .iter()
        .filter(|event| event.data["event"] != "runtime_closed")
        .cloned()
        .collect();
    altered.extend(inspection(&events));
    let records = Records::replay(&altered).expect("read-only inspection");
    let record = records.0.values().next().expect("root");
    assert_eq!(
        (
            record.execution_clean,
            record.can_resume(),
            record.can_clean()
        ),
        (false, false, false)
    );
}

#[tokio::test]
async fn deleting_the_derived_state_cache_does_not_create_a_new_native_attempt() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    std::fs::remove_file(fixture.directory().join("state.json")).expect("discard cache");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepStarted)
        ),
        (1, 1, 1)
    );
}
