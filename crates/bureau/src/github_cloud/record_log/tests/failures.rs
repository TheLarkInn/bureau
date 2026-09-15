use std::io;

use serde_json::json;

use super::fixture::{Fixture, REQUEST, observation, prepared, selected, start, task};
use crate::github_cloud::record_log::{Error, Log, read_state};
use crate::github_cloud::records::{Dispatch, Record};
use crate::process::{REDACTED, Secret};
use crate::runlog::EVENTS_FILE;

#[test]
fn write_error_is_visible_and_does_not_commit_in_memory_state() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let state = log.state().clone();
    let before = fixture.raw();
    let result = log.append_with(&fixture.owner, &prepared(), |_, _| {
        Err(io::Error::other("injected write failure"))
    });
    assert_eq!(
        (
            matches!(result, Err(Error::Io(_))),
            log.state(),
            fixture.raw()
        ),
        (true, &state, before)
    );
    assert!(log.append(&fixture.owner, &prepared()).is_err());
}

#[test]
fn a_failed_sync_receipt_keeps_durable_send_intent_and_requires_reopen() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let state = log.state().clone();
    let result = log.append_with(&fixture.owner, &prepared(), |writer, data| {
        writer.append(crate::runlog::EventKind::GitHubCloud, data)?;
        Err(io::Error::other("injected completion sync failure"))
    });
    let unchanged = log.state().clone();
    let closed = log.close();
    let persisted = read_state(fixture.root(), REQUEST).expect("durable intent");
    assert_eq!(
        (
            matches!(result, Err(Error::Io(_))),
            unchanged,
            closed.is_err(),
            persisted.dispatch
        ),
        (true, state, true, Dispatch::Prepared)
    );
}

#[test]
fn readback_failure_does_not_report_an_in_memory_completion() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    let state = log.state().clone();
    let result = log.append_with(&fixture.owner, &prepared(), |writer, data| {
        let sequence = writer.append(crate::runlog::EventKind::GitHubCloud, data)?;
        std::fs::rename(
            writer.dir().join(EVENTS_FILE),
            writer.dir().join("events.saved"),
        )?;
        Ok(sequence)
    });
    assert_eq!(
        (matches!(result, Err(Error::Io(_))), log.state()),
        (true, &state)
    );
    let dir = fixture.dir();
    std::fs::rename(dir.join("events.saved"), dir.join(EVENTS_FILE)).expect("restore log");
    assert_eq!(
        read_state(fixture.root(), REQUEST)
            .expect("receipt")
            .dispatch,
        Dispatch::Prepared
    );
}

#[test]
fn rejected_transition_does_not_disable_a_healthy_writer() {
    let fixture = Fixture::new();
    let mut log = fixture.create(&[]);
    assert!(log.append(&fixture.owner, &Record::Accepted).is_err());
    log.append(&fixture.owner, &prepared())
        .expect("prepare after invalid transition");
    log.append(&fixture.owner, &Record::Accepted)
        .expect("accept");
    assert_eq!(log.state().dispatch, Dispatch::Accepted);
    log.close().expect("close");
}

#[test]
fn creation_propagates_filesystem_failure() {
    let fixture = Fixture::new();
    let root = fixture.root().join("not-a-directory");
    std::fs::write(&root, "file").expect("blocking file");
    let result = Log::create(&root, start(), &[], &fixture.owner);
    assert!(matches!(result, Err(Error::Io(_))));
}

fn secret_receipt(fixture: &Fixture, secret: &str) -> Log {
    let mut start = start();
    start.definition["body"] = json!(secret);
    let mut log = Log::create(
        fixture.root(),
        start,
        &[Secret::new(secret)],
        &fixture.owner,
    )
    .expect("secret-bearing definition");
    log.append(&fixture.owner, &prepared()).expect("prepare");
    log.append(
        &fixture.owner,
        &Record::Uncertain {
            message: secret.to_owned(),
        },
    )
    .expect("uncertain");
    log.append(&fixture.owner, &selected()).expect("select");
    let mut observed = task();
    observed["body"] = json!(secret);
    let events = Some(vec![json!({"id": "event:one", "body": secret})]);
    log.append(&fixture.owner, &observation(observed, events))
        .expect("observe");
    log
}

#[test]
fn secrets_are_absent_from_disk_and_the_serialized_receipt() {
    let fixture = Fixture::new();
    let secret = "test-secret-\"quoted\"\nΩ";
    let log = secret_receipt(&fixture, secret);
    let shown = serde_json::to_string(log.state()).expect("receipt JSON");
    let state = log.state().clone();
    assert_eq!(
        (
            fixture.raw().contains(secret),
            shown.contains(secret),
            shown.contains(REDACTED)
        ),
        (false, false, true)
    );
    log.close().expect("close");
    let rebuilt = read_state(fixture.root(), REQUEST).expect("replay");
    assert_eq!(rebuilt, state);
    assert_eq!(rebuilt.dispatch_message.as_deref(), Some(REDACTED));
}

#[test]
fn secret_bearing_json_keys_are_rejected_before_creation() {
    let fixture = Fixture::new();
    let secret = "test-only-key-secret";
    let mut start = start();
    start.definition[secret] = json!("untrusted data");
    let result = Log::create(
        fixture.root(),
        start,
        &[Secret::new(secret)],
        &fixture.owner,
    );
    assert_eq!((result.is_err(), fixture.dir().exists()), (true, false));
}

#[test]
fn scrubbing_must_not_change_immutable_scope_identity() {
    let fixture = Fixture::new();
    let mut start = start();
    let secret = "test-only-scope-secret";
    start.scope.registry_name = secret.to_owned();
    let result = Log::create(
        fixture.root(),
        start,
        &[Secret::new(secret)],
        &fixture.owner,
    );
    assert_eq!((result.is_err(), fixture.dir().exists()), (true, false));
}

#[test]
fn scrubbing_must_not_change_a_selected_task_identity() {
    let fixture = Fixture::new();
    let secret = "task-only-secret";
    let mut log = fixture.create(&[Secret::new(secret)]);
    let before = fixture.raw();
    let record = Record::TaskSelected {
        task_id: secret.to_owned(),
    };
    let result = log.append(&fixture.owner, &record);
    assert_eq!(
        (
            result.is_err(),
            fixture.raw(),
            log.state().task_id.is_none()
        ),
        (true, before, true)
    );
    log.close().expect("close");
}
