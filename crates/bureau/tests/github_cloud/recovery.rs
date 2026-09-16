use bureau::forge::github::cloud::{AutomationId, TaskId};
use bureau::github_cloud::{Dispatch, Error, Record, dispatch, read_state, refresh, track};
use serde_json::json;

use super::control_support::{Fixture, posts, task_replies};
use super::support;

fn prepared(fixture: &Fixture) {
    let (owner, mut log) = fixture.log("receipt");
    log.append(
        &owner,
        &Record::Prepared {
            event: "manual".to_owned(),
        },
    )
    .expect("intent");
    owner.release().expect("release");
}

#[tokio::test]
async fn restart_after_intent_does_not_reissue_the_post() {
    let fixture = Fixture::new(vec![]).await;
    prepared(&fixture);
    let state = dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("replay");
    assert_eq!(
        (state.dispatch, fixture.fake.requests().len()),
        (Dispatch::Prepared, 1)
    );
}

#[tokio::test]
async fn cache_deletion_cannot_erase_submission_uncertainty() {
    let fixture = Fixture::new(vec![]).await;
    prepared(&fixture);
    let cache = fixture.root.join("receipt/state.json");
    std::fs::write(&cache, "{}").expect("untrusted cache");
    std::fs::remove_file(cache).expect("remove derived cache");
    let state = dispatch(&fixture.control(), "receipt", &support::automation())
        .await
        .expect("log replay");
    assert_eq!(state.dispatch, Dispatch::Prepared);
}

#[tokio::test]
async fn mismatching_request_key_is_not_a_new_operation() {
    let fixture = Fixture::new(vec![]).await;
    prepared(&fixture);
    let other = AutomationId::try_from("automation-other".to_owned()).expect("other ID");
    let result = dispatch(&fixture.control(), "receipt", &other).await;
    assert!(matches!(result, Err(Error::Selection(_))) && posts(&fixture.fake.requests()) == 0);
}

#[tokio::test]
async fn an_uncertain_receipt_can_only_gain_an_explicit_task_selection() {
    let fixture = Fixture::new(task_replies()).await;
    prepared(&fixture);
    let state = track(
        &fixture.control(),
        "receipt",
        &support::automation(),
        &support::task_id(),
    )
    .await
    .expect("selection");
    let serialized = serde_json::to_value(&state).expect("state JSON");
    assert!(
        state.dispatch == Dispatch::Prepared
            && state.task_id.as_deref() == Some("task-1")
            && serialized["task_correlation"] == json!("operator_selected_unproven")
    );
}

#[tokio::test]
async fn a_selected_task_cannot_silently_change() {
    let fixture = Fixture::new(task_replies()).await;
    track(
        &fixture.control(),
        "tracked",
        &support::automation(),
        &support::task_id(),
    )
    .await
    .expect("first selection");
    let other = TaskId::try_from("task-other".to_owned()).expect("other ID");
    let result = track(
        &fixture.control(),
        "tracked",
        &support::automation(),
        &other,
    )
    .await;
    assert!(matches!(result, Err(Error::Selection(_))) && fixture.fake.requests().len() == 4);
}

fn refresh_replies()
-> Vec<Result<bureau::forge::github::cloud::Response, bureau::forge::github::cloud::Error>> {
    let mut replies = task_replies();
    replies.extend(support::definition_replies());
    let mut earlier = support::task();
    earlier["state"] = json!("completed");
    replies.push(support::response(200, &earlier));
    replies.push(support::response(
        200,
        &json!({"events": [{"type": "session.idle"}], "total": 1}),
    ));
    replies.push(support::response(200, &support::task()));
    replies
}

#[tokio::test]
async fn fresh_waiting_state_wins_over_older_completion_evidence() {
    let fixture = Fixture::new(refresh_replies()).await;
    track(
        &fixture.control(),
        "tracked",
        &support::automation(),
        &support::task_id(),
    )
    .await
    .expect("selection");
    let state = refresh(&fixture.control(), "tracked", true)
        .await
        .expect("observation");
    assert_eq!(
        (
            state.task.as_ref().and_then(|task| task["state"].as_str()),
            state.events.len(),
            state.events_reported_total,
            posts(&fixture.fake.requests())
        ),
        (Some("waiting_for_user"), 1, Some(1), 0)
    );
}

#[tokio::test]
async fn a_bodyless_or_torn_receipt_cannot_be_redispatched() {
    let fixture = Fixture::new(vec![]).await;
    std::fs::create_dir_all(fixture.root.join("receipt")).expect("interrupted directory");
    std::fs::write(fixture.root.join("receipt/events.jsonl"), "{\"").expect("torn header");
    let result = dispatch(&fixture.control(), "receipt", &support::automation()).await;
    assert!(
        result.is_err()
            && read_state(&fixture.root, "receipt").is_err()
            && posts(&fixture.fake.requests()) == 0
    );
}
