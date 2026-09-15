use serde_json::{Value, json};

use super::fixture::{AUTOMATION, Fixture, REQUEST, observation, selected, task};
use crate::github_cloud::record_log::Log;
use crate::github_cloud::records::Record;

fn rejected(log: &mut Log, fixture: &Fixture, task: Value, events: Option<Vec<Value>>) {
    let before = log.state().clone();
    let bytes = fixture.raw();
    let result = log.append(&fixture.owner, &observation(task, events));
    assert_eq!(
        (result.is_err(), log.state(), fixture.raw()),
        (true, &before, bytes)
    );
}

fn selected_log() -> (Fixture, Log) {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    log.append(&fixture.owner, &selected())
        .expect("select task");
    (fixture, log)
}

fn observed_log() -> (Fixture, Log) {
    let (fixture, mut log) = selected_log();
    let events = Some(vec![
        json!({"id": "event:one", "type": "future_remote_event"}),
    ]);
    log.append(&fixture.owner, &observation(task(), events))
        .expect("observe");
    (fixture, log)
}

fn old_observation() -> (Fixture, Log) {
    let (fixture, log) = observed_log();
    log.close().expect("close");
    let mut events = fixture.events();
    let observation = events.last_mut().expect("observation");
    observation.at_ms = 17;
    observation.data["record"]["reported_total"] = json!(99);
    fixture.write_events(&events);
    let log = Log::open(fixture.root(), REQUEST, &[], &fixture.owner).expect("open");
    (fixture, log)
}

#[test]
fn task_identity_fields_are_required_non_null_and_exact() {
    let (fixture, mut log) = selected_log();
    for field in ["id", "automation_id"] {
        for value in [Value::Null, json!(42), json!("another"), json!({})] {
            let mut observed = task();
            observed[field] = value;
            rejected(&mut log, &fixture, observed, None);
        }
    }
    log.close().expect("close");
}

#[test]
fn non_object_and_missing_task_identities_are_rejected() {
    let (fixture, mut log) = selected_log();
    for observed in [
        Value::Null,
        json!([]),
        json!("task"),
        json!({}),
        json!({"id": "task:one"}),
    ] {
        rejected(&mut log, &fixture, observed, None);
    }
    log.close().expect("close");
}

#[test]
fn every_session_must_belong_to_the_selected_task() {
    let (fixture, mut log) = selected_log();
    for sessions in [
        Value::Null,
        json!({}),
        json!([null]),
        json!([{}]),
        json!([{"task_id": null}]),
        json!([{"task_id": "another"}]),
    ] {
        let mut observed = task();
        observed["sessions"] = sessions;
        rejected(&mut log, &fixture, observed, None);
    }
    log.close().expect("close");
}

#[test]
fn all_sessions_are_checked_not_only_the_first() {
    let (fixture, mut log) = selected_log();
    let mut observed = task();
    observed["sessions"][1]["task_id"] = json!("another");
    rejected(&mut log, &fixture, observed, None);
    log.close().expect("close");
}

#[test]
fn missing_sessions_is_allowed_but_null_is_not_defaulted() {
    let (fixture, mut log) = selected_log();
    let observed = json!({"id": "task:one", "automation_id": AUTOMATION, "state": "unknown"});
    log.append(&fixture.owner, &observation(observed.clone(), None))
        .expect("observe missing sessions");
    assert_eq!(log.state().task.as_ref(), Some(&observed));
    log.close().expect("close");
}

#[test]
fn event_values_must_be_objects() {
    let (fixture, mut log) = selected_log();
    for event in [Value::Null, json!(42), json!("event"), json!([])] {
        rejected(
            &mut log,
            &fixture,
            task(),
            Some(vec![json!({"id": "ok"}), event]),
        );
    }
    log.close().expect("close");
}

#[test]
fn open_remote_states_and_session_identities_remain_raw_data() {
    let (fixture, mut log) = selected_log();
    let event = json!({"type": "future_kind", "payload": {"schema": "v2", "outcome": "success"}});
    log.append(
        &fixture.owner,
        &observation(task(), Some(vec![event.clone()])),
    )
    .expect("observe");
    assert_eq!(
        (&log.state().task, &log.state().events),
        (&Some(task()), &vec![event])
    );
    log.close().expect("close");
}

#[test]
fn task_ids_are_opaque_data_not_local_request_keys() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let task_id = "task/remote:1.with-punctuation";
    log.append(
        &fixture.owner,
        &Record::TaskSelected {
            task_id: task_id.to_owned(),
        },
    )
    .expect("opaque task selection");
    let observed = json!({"id": task_id, "automation_id": AUTOMATION});
    log.append(&fixture.owner, &observation(observed, None))
        .expect("opaque task observation");
    assert_eq!(log.state().task_id.as_deref(), Some(task_id));
    log.close().expect("close");
}

#[test]
fn observation_timestamps_come_from_the_durable_outer_event() {
    let (fixture, log) = observed_log();
    let at_ms = fixture.events().last().expect("observation").at_ms;
    assert_eq!(
        (
            log.state().observed_at_ms,
            log.state().events_observed_at_ms
        ),
        (Some(at_ms), Some(at_ms))
    );
    log.close().expect("close");
}

#[test]
fn reading_only_the_task_does_not_relabel_old_events() {
    let (fixture, mut log) = old_observation();
    let previous = log.state().events.clone();
    log.append(&fixture.owner, &observation(task(), None))
        .expect("task-only read");
    let at_ms = fixture
        .events()
        .last()
        .expect("task-only observation")
        .at_ms;
    assert_eq!(
        (
            log.state().observed_at_ms,
            log.state().events_observed_at_ms,
            log.state().events_reported_total,
            &log.state().events
        ),
        (Some(at_ms), Some(17), Some(99), &previous)
    );
    log.close().expect("close");
}

#[test]
fn an_explicit_empty_event_read_replaces_old_events_and_timestamp() {
    let (fixture, mut log) = old_observation();
    log.append(&fixture.owner, &observation(task(), Some(Vec::new())))
        .expect("empty event read");
    let at_ms = fixture.events().last().expect("observation").at_ms;
    assert_eq!(
        (
            log.state().events.len(),
            log.state().events_observed_at_ms,
            log.state().events_reported_total
        ),
        (0, Some(at_ms), None)
    );
    log.close().expect("close");
}

#[test]
fn event_totals_remain_observations_not_snapshot_guarantees() {
    let (fixture, mut log) = selected_log();
    for total in [1, 200, 0] {
        let record = Record::Observed {
            task: task(),
            events: Some(vec![json!({"id": "event:one"})]),
            reported_total: Some(total),
        };
        log.append(&fixture.owner, &record)
            .expect("changing observed total");
        assert_eq!(log.state().events_reported_total, Some(total));
    }
    log.close().expect("close");
}

#[test]
fn an_event_total_without_new_events_is_rejected() {
    let (fixture, mut log) = selected_log();
    let before = log.state().clone();
    let record = Record::Observed {
        task: task(),
        events: None,
        reported_total: Some(99),
    };
    let result = log.append(&fixture.owner, &record);
    assert_eq!((result.is_err(), log.state()), (true, &before));
    log.close().expect("close");
}
