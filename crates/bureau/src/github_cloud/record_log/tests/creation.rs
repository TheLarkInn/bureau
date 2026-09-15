use serde_json::{Value, json};

use super::fixture::{Fixture, REQUEST, prepared, selected, start};
use crate::github_cloud::record_log::{Log, read_state, validate_key};
use crate::github_cloud::records::{Dispatch, Scope, Start};
use crate::runlog::{EVENTS_FILE, STATE_FILE};

#[test]
fn local_keys_accept_bounded_safe_components() {
    let valid = ["request-1".to_owned(), "_".to_owned(), "A".repeat(128)];
    for key in valid {
        assert!(validate_key(&key).is_ok());
    }
}

#[test]
fn local_keys_reject_traversal_controls_and_oversized_components() {
    let invalid = [
        "",
        ".",
        "..",
        "../request",
        "a/b",
        "a\\b",
        "a:b",
        "request\n",
        "request\0",
        "é",
        " a",
    ];
    let oversized = "x".repeat(129);
    for key in invalid.into_iter().chain([oversized.as_str()]) {
        assert!(validate_key(key).is_err());
    }
}

#[test]
fn created_receipt_is_events_only_and_acceptance_free() {
    let fixture = Fixture::new();
    let log = fixture.create(&[]);
    let state = log.state();
    let record = &fixture.events()[0].data;
    assert_eq!(
        (
            state.dispatch,
            state.task_id.as_deref(),
            state.task_correlation,
            record["schema"].as_str()
        ),
        (Dispatch::NotSubmitted, None, None, Some("github_cloud_v1"))
    );
    let entries = std::fs::read_dir(fixture.dir())
        .expect("run directory")
        .count();
    assert_eq!(entries, 1);
    log.close().expect("close");
}

#[test]
fn create_refuses_an_existing_receipt_without_rewriting_it() {
    let fixture = Fixture::new();
    fixture.create(&[]).close().expect("close");
    let before = fixture.raw();
    let result = Log::create(fixture.root(), start(), &[], &fixture.owner);
    assert_eq!((result.is_err(), fixture.raw()), (true, before));
}

#[test]
fn replay_needs_no_derived_cache() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    log.append(&fixture.owner, &prepared()).expect("prepare");
    log.append(&fixture.owner, &selected()).expect("select");
    let expected = log.state().clone();
    log.close().expect("close");
    let cache = fixture.dir().join(STATE_FILE);
    std::fs::write(&cache, "not authoritative").expect("cache fixture");
    assert_eq!(read_state(fixture.root(), REQUEST).expect("read"), expected);
    std::fs::remove_file(cache).expect("delete cache");
    let reopened = Log::open(fixture.root(), REQUEST, &[], &fixture.owner).expect("reopen");
    assert_eq!(reopened.state(), &expected);
    reopened.close().expect("close");
}

#[test]
fn scope_and_start_reject_unknown_and_missing_fields() {
    let mut scope = serde_json::to_value(start().scope).expect("scope JSON");
    scope["unknown"] = json!(true);
    assert!(serde_json::from_value::<Scope>(scope).is_err());
    let mut start = serde_json::to_value(start()).expect("start JSON");
    start["unknown"] = json!(true);
    assert!(serde_json::from_value::<Start>(start).is_err());
}

#[test]
fn missing_or_null_scope_identity_is_not_defaulted() {
    for field in [
        "repo",
        "registry_name",
        "credential_reference",
        "principal_id",
        "principal_login",
    ] {
        let mut value = serde_json::to_value(start().scope).expect("scope JSON");
        value[field] = Value::Null;
        assert!(serde_json::from_value::<Scope>(value).is_err());
    }
}

#[test]
fn committed_config_identity_also_rejects_unknown_fields() {
    let mut value = serde_json::to_value(start().scope).expect("scope JSON");
    value["config_source"]["extra"] = json!(true);
    assert!(serde_json::from_value::<Scope>(value).is_err());
}

#[test]
fn malformed_start_is_rejected_before_file_creation() {
    let fixture = Fixture::new();
    let mut value = start();
    value.scope.principal_id = 0;
    let result = Log::create(fixture.root(), value, &[], &fixture.owner);
    assert_eq!(
        (result.is_err(), fixture.dir().join(EVENTS_FILE).exists()),
        (true, false)
    );
}
