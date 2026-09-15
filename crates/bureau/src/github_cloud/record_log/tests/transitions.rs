use serde_json::json;

use super::fixture::{Fixture, REQUEST, TASK, observation, prepared, selected, task};
use crate::github_cloud::record_log::{Log, read_state};
use crate::github_cloud::records::{Dispatch, Record};

fn uncertain() -> Record {
    Record::Uncertain {
        message: "lost response".to_owned(),
    }
}

fn completions() -> [Record; 3] {
    [
        Record::Accepted,
        Record::Rejected {
            message: "rejected".to_owned(),
        },
        uncertain(),
    ]
}

#[test]
fn completion_requires_prepared_and_preserves_state_on_error() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let state = log.state().clone();
    let raw = fixture.raw();
    for record in completions() {
        let result = log.append(&fixture.owner, &record);
        assert_eq!(
            (result.is_err(), log.state(), fixture.raw()),
            (true, &state, raw.clone())
        );
    }
    log.close().expect("close");
}

#[test]
fn a_prepared_submission_has_only_one_completion() {
    for record in completions() {
        let fixture = Fixture::new();
        let mut log = fixture.create(&[]);
        log.append(&fixture.owner, &prepared()).expect("prepare");
        log.append(&fixture.owner, &record).expect("complete");
        let state = log.state().clone();
        let repeated = log.append(&fixture.owner, &record);
        let prepared = log.append(&fixture.owner, &prepared());
        assert_eq!(
            (repeated.is_err(), prepared.is_err(), log.state()),
            (true, true, &state)
        );
        log.close().expect("close");
    }
}

#[test]
fn interrupted_prepared_history_remains_possibly_sent_after_restart() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    log.append(&fixture.owner, &prepared()).expect("prepare");
    log.close().expect("close");
    let before = fixture.raw();
    let mut reopened = Log::open(fixture.root(), REQUEST, &[], &fixture.owner).expect("open");
    let repeated = reopened.append(&fixture.owner, &prepared());
    assert_eq!(
        (reopened.state().dispatch, repeated.is_err(), fixture.raw()),
        (Dispatch::Prepared, true, before)
    );
    reopened.close().expect("close");
}

#[test]
fn explicit_task_selection_never_proves_submission_correlation() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    log.append(&fixture.owner, &prepared()).expect("prepare");
    log.append(&fixture.owner, &uncertain()).expect("uncertain");
    log.append(&fixture.owner, &selected()).expect("select");
    let value = serde_json::to_value(log.state()).expect("state JSON");
    assert_eq!(
        (
            value["dispatch"].as_str(),
            value["task_id"].as_str(),
            value["task_correlation"].as_str()
        ),
        (
            Some("uncertain"),
            Some(TASK),
            Some("operator_selected_unproven")
        )
    );
    assert!(
        ["outcome", "cost_usd", "result"]
            .iter()
            .all(|key| value.get(key).is_none())
    );
    log.close().expect("close");
}

#[test]
fn selected_task_is_immutable_and_tracking_cannot_become_dispatch() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    log.append(&fixture.owner, &selected()).expect("select");
    let state = log.state().clone();
    log.append(&fixture.owner, &selected())
        .expect("same selection");
    let changed = log.append(
        &fixture.owner,
        &Record::TaskSelected {
            task_id: "other".to_owned(),
        },
    );
    let dispatch = log.append(&fixture.owner, &prepared());
    assert_eq!(
        (changed.is_err(), dispatch.is_err(), log.state()),
        (true, true, &state)
    );
    log.close().expect("close");
}

#[test]
fn observations_require_explicit_selection() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let result = log.append(&fixture.owner, &observation(task(), None));
    assert_eq!((result.is_err(), log.state().task.is_none()), (true, true));
    log.close().expect("close");
}

#[test]
fn dispatch_events_are_exactly_manual_or_interval() {
    for event in ["manual", "interval", "schedule", "", "push"] {
        let fixture = Fixture::new();
        let mut log = fixture.create(&[]);
        let result = log.append(
            &fixture.owner,
            &Record::Prepared {
                event: event.to_owned(),
            },
        );
        assert_eq!(result.is_ok(), matches!(event, "manual" | "interval"));
        log.close().expect("close");
    }
}

#[test]
fn dispatch_labels_match_their_wire_names() {
    for dispatch in [
        Dispatch::NotSubmitted,
        Dispatch::Prepared,
        Dispatch::Accepted,
        Dispatch::Rejected,
        Dispatch::Uncertain,
    ] {
        assert_eq!(
            serde_json::to_value(dispatch).expect("JSON"),
            json!(dispatch.label())
        );
    }
}

#[test]
fn acceptance_does_not_invent_a_task() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    log.append(&fixture.owner, &prepared()).expect("prepare");
    log.append(&fixture.owner, &Record::Accepted)
        .expect("accept");
    log.close().expect("close");
    let state = read_state(fixture.root(), REQUEST).expect("receipt");
    assert_eq!(
        (state.dispatch, state.task_id, state.task),
        (Dispatch::Accepted, None, None)
    );
}
