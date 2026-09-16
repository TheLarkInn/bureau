use bureau::forge::github::cloud::Error;
use serde_json::{Value, json};

use super::support::{self, Fake};

const NEXT: &str = "https://api.github.com/cmc_internal/api/agents/repos/example/project/automations/v2?page=2&per_page=100";

#[tokio::test]
async fn inventory_continuations_stay_on_the_original_scope() {
    let page = json!({"automations": [support::summary()]});
    let (client, fake) = Fake::client(vec![
        support::linked(&page, NEXT),
        support::response(200, &page),
    ]);
    let items = client.automations(&support::repo()).await.expect("pages");
    let requests = fake.requests();
    assert_eq!(
        (items.len(), requests.len(), requests[1].url().as_str()),
        (1, 2, NEXT)
    );
}

#[tokio::test]
async fn foreign_links_never_receive_credentials() {
    let targets = [
        NEXT.replace("api.github.com", "untrusted.example"),
        NEXT.replace("example/project", "different/repo"),
        NEXT.replace("page=2", "page=1"),
        format!("{NEXT}&extra=scope"),
        format!("{NEXT}&page=2"),
    ];
    for target in targets {
        let (client, fake) =
            Fake::client(vec![support::linked(&json!({"automations": []}), &target)]);
        let result = client.automations(&support::repo()).await;
        assert!(matches!(result, Err(Error::Incomplete(_))) && fake.requests().len() == 1);
    }
}

fn full_task_page() -> Value {
    let tasks: Vec<Value> = (0..100).map(|_| support::task()).collect();
    json!({"tasks": tasks})
}

#[tokio::test]
async fn task_history_uses_fixed_filters_and_short_page_termination() {
    let mut replies = support::definition_replies();
    replies.extend([
        support::response(200, &full_task_page()),
        support::response(200, &json!({"tasks": []})),
    ]);
    let (client, fake) = Fake::client(replies);
    let tasks = client
        .tasks(&support::repo(), &support::automation())
        .await
        .expect("tasks");
    let requests = fake.requests();
    let query = support::queries(&requests[3]);
    assert_eq!(
        (
            tasks.len(),
            query.get("page").map(String::as_str),
            query.get("is_archived").map(String::as_str),
            query.contains_key("page_limit")
        ),
        (1, Some("2"), Some("false"), false)
    );
}

#[tokio::test]
async fn the_page_cap_is_not_reported_as_complete_history() {
    let mut replies = support::definition_replies();
    replies.extend((0..100).map(|_| support::response(200, &full_task_page())));
    let (client, fake) = Fake::client(replies);
    let result = client.tasks(&support::repo(), &support::automation()).await;
    assert!(matches!(result, Err(Error::Incomplete(_))) && fake.requests().len() == 102);
}

#[tokio::test]
async fn event_totals_do_not_pretend_to_be_a_snapshot_cursor() {
    let full: Vec<Value> = (0..100).map(|index| json!({"unknown": index})).collect();
    let replies = vec![
        support::response(200, &json!({"events": full, "total": 100})),
        support::response(200, &json!({"events": [], "total": 101})),
    ];
    let (client, fake) = Fake::client(replies);
    let result = client.events(&support::task_id()).await.expect("events");
    assert_eq!(
        (
            result.events.len(),
            result.reported_total,
            fake.requests().len()
        ),
        (100, Some(101), 2)
    );
}
