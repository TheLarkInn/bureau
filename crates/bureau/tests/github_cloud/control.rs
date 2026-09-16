use bureau::config::Access;
use bureau::forge::github::cloud;
use bureau::github_cloud::{Dispatch, Error, dispatch, read_state, track};
use serde_json::json;

use super::control_support::{Fixture, accepted_replies, posts, task_replies};
use super::support;

#[tokio::test]
async fn accepted_receipt_has_no_automatic_task_identity_or_worktree() {
    let fixture = Fixture::new(accepted_replies()).await;
    let state = dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("submission");
    let disk = read_state(&fixture.root, "receipt").expect("replay");
    assert_eq!(
        (
            state.dispatch,
            state.task_id,
            disk.dispatch,
            posts(&fixture.fake.requests()),
            fixture.root.join("receipt/wt").exists()
        ),
        (Dispatch::Accepted, None, Dispatch::Accepted, 1, false)
    );
}

#[tokio::test]
async fn repeating_a_key_never_dispatches_twice() {
    let fixture = Fixture::new(accepted_replies()).await;
    dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("first");
    let second = dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("existing");
    assert_eq!(
        (second.dispatch, posts(&fixture.fake.requests())),
        (Dispatch::Accepted, 1)
    );
}

#[tokio::test]
async fn lost_response_remains_durable_and_is_not_retried() {
    let mut replies = support::definition_replies();
    replies.push(Err(cloud::Error::Transport("response lost".to_owned())));
    let fixture = Fixture::new(replies).await;
    let first = dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("uncertain receipt");
    let again = dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("replay");
    assert_eq!(
        (
            first.dispatch,
            again.dispatch,
            posts(&fixture.fake.requests())
        ),
        (Dispatch::Uncertain, Dispatch::Uncertain, 1)
    );
}

#[tokio::test]
async fn explicit_rejection_is_a_receipt_not_a_successful_task() {
    let mut replies = support::definition_replies();
    replies.push(support::response(403, &json!({"message": support::TOKEN})));
    let fixture = Fixture::new(replies).await;
    let state = dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("rejected receipt");
    let disk =
        serde_json::to_string(&read_state(&fixture.root, "receipt").expect("disk")).expect("JSON");
    assert_eq!(
        (state.dispatch, state.task_id, disk.contains(support::TOKEN)),
        (Dispatch::Rejected, None, false)
    );
}

#[tokio::test]
async fn narrower_registry_access_cannot_dispatch() {
    for access in [Access::Read, Access::Pr] {
        let fixture = Fixture::with_access(vec![], access).await;
        let result = dispatch(&fixture.control(), "receipt", &support::automation()).await;
        assert!(
            matches!(result, Err(Error::Unsupported(_))) && posts(&fixture.fake.requests()) == 0
        );
    }
}

#[tokio::test]
async fn a_disabled_definition_never_creates_a_submission_intent() {
    let mut definition = support::definition();
    definition["disabled"] = json!(true);
    let mut replies = support::definition_replies();
    replies[1] = support::response(200, &definition);
    let fixture = Fixture::new(replies).await;
    let result = dispatch(&fixture.control(), "receipt", &support::automation()).await;
    assert!(
        result.is_err()
            && !fixture.root.join("receipt").exists()
            && posts(&fixture.fake.requests()) == 0
    );
}

#[tokio::test]
async fn tracking_is_explicit_and_does_not_submit() {
    let fixture = Fixture::new(task_replies()).await;
    let state = track(
        &fixture.control(),
        "tracked",
        &support::automation(),
        &support::task_id(),
    )
    .await
    .expect("track");
    assert_eq!(
        (
            state.dispatch,
            state.task_id.as_deref(),
            posts(&fixture.fake.requests())
        ),
        (Dispatch::NotSubmitted, Some("task-1"), 0)
    );
}

#[tokio::test]
async fn archived_tasks_cannot_be_newly_attached() {
    let mut task = support::task();
    task["archived_at"] = json!("2026-01-02T00:00:00Z");
    let mut replies = support::definition_replies();
    replies.push(support::response(200, &task));
    let fixture = Fixture::new(replies).await;
    let result = track(
        &fixture.control(),
        "tracked",
        &support::automation(),
        &support::task_id(),
    )
    .await;
    assert!(matches!(result, Err(Error::Selection(_))) && !fixture.root.join("tracked").exists());
}

#[tokio::test]
async fn active_client_secret_is_scrubbed_without_an_optional_extra_scrub_list() {
    let mut definition = support::definition();
    definition["prompt"] = json!(support::TOKEN);
    let mut replies = accepted_replies();
    replies[1] = support::response(200, &definition);
    let fixture = Fixture::new(replies).await;
    dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("submission");
    let text = std::fs::read_to_string(fixture.root.join("receipt/events.jsonl")).expect("events");
    assert!(!text.contains(support::TOKEN));
}
