use bureau::forge::github::cloud::{Definition, DispatchEvent, Error, Task};
use serde_json::{Value, json};

use super::support::{self, Fake};

fn with_field(name: &str, value: Value) -> Value {
    let mut definition = support::definition();
    definition[name] = value;
    definition
}

#[test]
fn trigger_nullability_is_not_blanket_defaulting() {
    let cases = [
        (Value::Null, true),
        (json!({}), true),
        (json!({"interval": {"types": [], "minutes": 15}}), true),
        (json!({"interval": {}}), false),
        (json!({"interval": null}), false),
        (json!([]), false),
        (json!("manual"), false),
    ];
    for (value, valid) in cases {
        assert_eq!(
            serde_json::from_value::<Definition>(with_field("triggers", value)).is_ok(),
            valid
        );
    }
}

#[test]
fn run_now_matches_the_exact_single_schedule_rule() {
    let cases = [
        (json!({}), Some(DispatchEvent::Manual)),
        (
            json!({"interval": {"types": []}}),
            Some(DispatchEvent::Interval),
        ),
        (
            json!({"schedule": {"types": []}}),
            Some(DispatchEvent::Interval),
        ),
        (
            json!({"schedule": {"types": []}, "interval": {"types": []}}),
            None,
        ),
        (json!({"issues": {"types": ["opened"]}}), None),
        (json!({"workflow_run": {"types": ["completed"]}}), None),
    ];
    for (triggers, expected) in cases {
        let definition: Definition =
            serde_json::from_value(with_field("triggers", triggers)).expect("definition");
        assert_eq!(definition.dispatch_event().ok(), expected);
    }
}

#[tokio::test]
async fn disabled_dispatch_is_rejected_without_a_request() {
    let definition: Definition =
        serde_json::from_value(with_field("disabled", json!(true))).expect("definition");
    let (client, fake) = Fake::client(vec![]);
    let result = client.dispatch(&support::repo(), &definition).await;
    assert!(matches!(result, Err(Error::Unsupported(_))) && fake.requests().is_empty());
}

#[test]
fn explicit_null_is_not_a_boolean_default() {
    let fields = ["disabled", "require_actor_write_permission"];
    for field in fields {
        assert!(serde_json::from_value::<Definition>(with_field(field, Value::Null)).is_err());
    }
}

#[test]
fn absent_sessions_default_but_null_sessions_do_not() {
    let mut absent = support::task();
    absent
        .as_object_mut()
        .expect("task object")
        .remove("sessions");
    let mut null = absent.clone();
    null["sessions"] = Value::Null;
    let parsed = serde_json::from_value::<Task>(absent).expect("absent sessions");
    assert!(parsed.sessions.is_empty() && serde_json::from_value::<Task>(null).is_err());
}

#[tokio::test]
async fn task_and_status_strings_are_independent_observations() {
    let mut task = support::task();
    task["state"] = json!("previously-unseen-state");
    task["artifacts"] =
        json!([{"url": "https://untrusted.example/file", "schema": "v2", "outcome": "success"}]);
    let (client, fake) = Fake::client(vec![support::response(200, &task)]);
    let observed = client
        .task(&support::automation(), &support::task_id())
        .await
        .expect("task");
    assert_eq!(
        (
            observed.state.as_str(),
            observed.status.as_deref(),
            fake.requests().len()
        ),
        ("previously-unseen-state", Some("provider-defined"), 1)
    );
}

fn wrong_identity_cases() -> Vec<Value> {
    let mut id = support::task();
    id["id"] = json!("task-other");
    let mut automation = support::task();
    automation["automation_id"] = json!("automation-other");
    let mut missing = support::task();
    missing
        .as_object_mut()
        .expect("task")
        .remove("automation_id");
    let mut session = support::task();
    session["sessions"][0]["task_id"] = json!("task-other");
    vec![id, automation, missing, session]
}

#[tokio::test]
async fn exact_task_selection_rejects_unverifiable_identity() {
    for task in wrong_identity_cases() {
        let (client, _) = Fake::client(vec![support::response(200, &task)]);
        let result = client
            .task(&support::automation(), &support::task_id())
            .await;
        assert!(matches!(result, Err(Error::Identity(_))));
    }
}

#[tokio::test]
async fn raw_events_are_not_normalized_into_completion() {
    let events = json!([{"type": "unknown.remote", "data": {"sessionId": "session-1", "result": {"schema": "v2"}}}]);
    let (client, fake) = Fake::client(vec![support::response(
        200,
        &json!({"events": events, "total": 9}),
    )]);
    let observed = client.events(&support::task_id()).await.expect("events");
    assert_eq!(
        (
            json!(observed.events),
            observed.reported_total,
            fake.requests().len()
        ),
        (events, Some(9), 1)
    );
}

#[tokio::test]
async fn nonobject_event_values_are_rejected() {
    let (client, _) = Fake::client(vec![support::response(
        200,
        &json!({"events": ["not an event"]}),
    )]);
    let result = client.events(&support::task_id()).await;
    assert!(matches!(result, Err(Error::Response(_))));
}
