use serde_json::{Value, json};

use super::fixture::{Fixture, REQUEST, event, prepared, selected};
use crate::github_cloud::record_log::{Log, read_state};
use crate::github_cloud::records::Record;
use crate::runlog::EventKind;

fn prepared_history() -> Fixture {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    log.append(&fixture.owner, &prepared()).expect("prepare");
    log.close().expect("close");
    fixture
}

fn rejected_record(record: &Value) {
    let fixture = prepared_history();
    let mut events = fixture.events();
    events.push(event(
        2,
        json!({"schema": "github_cloud_v1", "record": record}),
    ));
    fixture.write_events(&events);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn sequences_must_start_at_zero_and_have_no_gaps_or_duplicates() {
    for (index, sequence) in [(0, 1), (1, 0), (1, 2), (1, u64::MAX)] {
        let fixture = prepared_history();
        let mut events = fixture.events();
        events[index].seq = sequence;
        fixture.write_events(&events);
        assert!(read_state(fixture.root(), REQUEST).is_err());
    }
}

#[test]
fn every_event_must_belong_to_the_cloud_family() {
    let fixture = prepared_history();
    let mut events = fixture.events();
    events[1].kind = EventKind::Output;
    fixture.write_events(&events);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn duplicate_outer_sequence_fields_are_not_silently_overwritten() {
    let fixture = prepared_history();
    let raw = fixture
        .raw()
        .replacen("\"seq\":1", "\"seq\":0,\"seq\":1", 1);
    fixture.write_raw(&raw);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn unknown_versions_are_rejected_even_on_the_final_record() {
    let fixture = prepared_history();
    let mut events = fixture.events();
    events[1].data["schema"] = json!("github_cloud_v2");
    fixture.write_events(&events);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn outer_unknown_kind_is_not_mistaken_for_a_torn_tail() {
    let fixture = prepared_history();
    let mut lines: Vec<_> = fixture
        .events()
        .iter()
        .map(|event| serde_json::to_value(event).expect("JSON"))
        .collect();
    lines[1]["kind"] = json!("future_cloud_family");
    let raw = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fixture.write_raw(&raw);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn unknown_record_kinds_and_fields_are_not_silently_ignored() {
    for record in [
        json!({"kind": "future_record"}),
        json!({"kind": "accepted", "scope": {"principal_id": 999}}),
        json!({"kind": "prepared", "event": "manual", "extra": true}),
    ] {
        rejected_record(&record);
    }
}

#[test]
fn the_envelope_rejects_unknown_fields() {
    let fixture = prepared_history();
    let mut events = fixture.events();
    events[0].data["new_scope"] = json!({});
    fixture.write_events(&events);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn creation_is_required_first_and_cannot_replace_immutable_identity() {
    let fixture = prepared_history();
    let events = fixture.events();
    let mut second = events[0].clone();
    second.seq = 2;
    second.data["record"]["start"]["scope"]["principal_id"] = json!(999);
    fixture.write_events(&[events[0].clone(), events[1].clone(), second]);
    assert!(read_state(fixture.root(), REQUEST).is_err());
    fixture.write_events(&[event(0, events[1].data.clone())]);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn the_stored_key_must_match_the_requested_directory() {
    let fixture = prepared_history();
    let mut events = fixture.events();
    events[0].data["record"]["start"]["request_id"] = json!("another-request");
    fixture.write_events(&events);
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn read_only_replay_ignores_but_never_repairs_a_torn_tail() {
    let fixture = prepared_history();
    let expected = read_state(fixture.root(), REQUEST).expect("initial state");
    let torn = format!(
        "{}{{\"seq\":2,\"kind\":\"github_cloud\",\"data\":",
        fixture.raw()
    );
    fixture.write_raw(&torn);
    assert_eq!(
        (
            read_state(fixture.root(), REQUEST).expect("torn read"),
            fixture.raw()
        ),
        (expected, torn)
    );
}

#[test]
fn only_a_validated_owned_open_repairs_a_torn_tail() {
    let fixture = prepared_history();
    let intact = fixture.raw();
    fixture.write_raw(&format!("{intact}{{\"seq\":2"));
    let mut log = Log::open(fixture.root(), REQUEST, &[], &fixture.owner).expect("owned open");
    assert_eq!(fixture.raw(), intact);
    log.append(&fixture.owner, &selected())
        .expect("append after repair");
    log.close().expect("close");
    assert_eq!(
        fixture
            .events()
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn a_complete_final_record_without_newline_is_not_fused_with_the_next_append() {
    let fixture = prepared_history();
    let raw = fixture.raw();
    fixture.write_raw(raw.trim_end());
    let mut log = Log::open(fixture.root(), REQUEST, &[], &fixture.owner).expect("open");
    log.append(&fixture.owner, &selected())
        .expect("append after missing newline");
    log.close().expect("close");
    assert_eq!(
        fixture
            .events()
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn malformed_interior_records_are_rejected_without_repair() {
    let fixture = prepared_history();
    let raw = fixture.raw().replacen('\n', "\n{\"seq\":\n", 1);
    fixture.write_raw(&raw);
    let result = Log::open(fixture.root(), REQUEST, &[], &fixture.owner);
    assert_eq!((result.is_err(), fixture.raw()), (true, raw));
}

#[test]
fn a_complete_malformed_final_record_is_not_a_torn_write() {
    let fixture = prepared_history();
    fixture.write_raw(&format!("{}not JSON\n", fixture.raw()));
    assert!(read_state(fixture.root(), REQUEST).is_err());
}

#[test]
fn replay_checks_transitions_and_observation_identity_again() {
    let fixture = prepared_history();
    let mut events = fixture.events();
    let data = json!({"schema": "github_cloud_v1", "record": {
        "kind": "observed", "task": {"id": "other", "automation_id": "other"}, "events": null
    }});
    events.push(event(2, data));
    fixture.write_events(&events);
    assert!(read_state(fixture.root(), REQUEST).is_err());
    rejected_record(&serde_json::to_value(prepared()).expect("prepared JSON"));
}

#[test]
fn empty_or_creation_free_logs_are_not_receipts() {
    let fixture = prepared_history();
    for text in ["", "\n", "{\"seq\":"] {
        fixture.write_raw(text);
        assert!(read_state(fixture.root(), REQUEST).is_err());
    }
}

#[test]
fn completion_replay_is_not_converted_to_a_pipeline_outcome() {
    let fixture = prepared_history();
    let mut log = Log::open(fixture.root(), REQUEST, &[], &fixture.owner).expect("open");
    log.append(&fixture.owner, &Record::Accepted)
        .expect("accept");
    log.close().expect("close");
    assert!(crate::runlog::replay(fixture.events()).is_none());
}
