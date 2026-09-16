use bureau::forge::github::GitHubForge;
use bureau::forge::github::cloud::{Definition, Error, RepositoryRef};
use bureau::process::Secret;
use serde_json::json;

use super::support::{self, Fake};

#[tokio::test]
async fn credential_identity_uses_bureaus_own_headers() {
    let (client, fake) = Fake::client(vec![support::response(
        200,
        &json!({"id": 17, "login": "runner"}),
    )]);
    let principal = client.authenticate("Runner").await.expect("identity");
    let requests = fake.requests();
    let request = &requests[0];
    assert_eq!(
        (
            principal.id,
            request.method().as_str(),
            request.url().as_str(),
            support::auth_headers(request)
        ),
        (17, "GET", "https://api.github.com/user", true)
    );
}

#[tokio::test]
async fn wrong_principal_never_counts_as_verified() {
    let (client, fake) = Fake::client(vec![support::response(
        200,
        &json!({"id": 17, "login": "different"}),
    )]);
    let result = client.authenticate("expected").await;
    assert!(matches!(result, Err(Error::Identity(_))) && fake.requests().len() == 1);
}

#[tokio::test]
async fn a_login_rename_does_not_change_the_recorded_numeric_principal() {
    let (client, _) = Fake::client(vec![support::response(
        200,
        &json!({"id": 17, "login": "renamed"}),
    )]);
    let principal = client.authenticate_id(17).await.expect("same principal");
    assert_eq!((principal.id, principal.login.as_str()), (17, "renamed"));
}

#[tokio::test]
async fn credential_rotation_to_another_principal_fails_closed() {
    let (client, _) = Fake::client(vec![support::response(
        200,
        &json!({"id": 18, "login": "runner"}),
    )]);
    let result = client.authenticate_id(17).await;
    assert!(matches!(result, Err(Error::Identity(_))));
}

#[tokio::test]
async fn automation_inventory_is_scoped_and_has_no_app_identity() {
    let (client, fake) = Fake::client(vec![support::response(
        200,
        &json!({"automations": [support::summary()]}),
    )]);
    let summaries = client
        .automations(&support::repo())
        .await
        .expect("inventory");
    let requests = fake.requests();
    let request = &requests[0];
    assert_eq!(
        (
            summaries.len(),
            request.url().as_str(),
            support::auth_headers(request)
        ),
        (
            1,
            "https://api.github.com/cmc_internal/api/agents/repos/example/project/automations/v2?page=1&per_page=100",
            true
        )
    );
}

#[tokio::test]
async fn optional_repository_is_not_fabricated() {
    let mut detail = support::definition();
    detail
        .as_object_mut()
        .expect("definition")
        .remove("repository");
    let mut replies = support::definition_replies();
    replies[1] = support::response(200, &detail);
    let (client, fake) = Fake::client(replies);
    let definition = client
        .definition(&support::repo(), &support::automation())
        .await
        .expect("detail");
    assert!(definition.repository.is_none() && fake.requests().len() == 2);
}

#[tokio::test]
async fn missing_membership_prevents_id_only_detail_lookup() {
    let (client, fake) = Fake::client(vec![support::response(200, &json!({"automations": []}))]);
    let result = client
        .definition(&support::repo(), &support::automation())
        .await;
    assert!(matches!(result, Err(Error::Identity(_))) && fake.requests().len() == 1);
}

#[tokio::test]
async fn cross_repo_definitions_fail_even_when_the_id_matches() {
    let mut detail = support::definition();
    detail["repository"]["owner"] = json!("different");
    let mut replies = support::definition_replies();
    replies[1] = support::response(200, &detail);
    let (client, _) = Fake::client(replies);
    let result = client
        .definition(&support::repo(), &support::automation())
        .await;
    assert!(matches!(result, Err(Error::Identity(_))));
}

fn assert_dispatch(requests: &[reqwest::Request]) {
    let request = &requests[0];
    assert_eq!(
        (
            requests.len(),
            request.method().as_str(),
            request.url().path(),
            support::body(request),
            support::auth_headers(request)
        ),
        (
            1,
            "POST",
            "/cmc_internal/api/agents/repos/example/project/automations/automation-1/tasks",
            json!({"event": "manual"}),
            true
        )
    );
}

#[tokio::test]
async fn every_2xx_dispatch_is_acceptance_only() {
    let definition: Definition = serde_json::from_value(support::definition()).expect("definition");
    for status in [200, 201, 202, 204, 299] {
        let (client, fake) = Fake::client(vec![support::response(
            status,
            &json!({"task_id": "not-a-contract"}),
        )]);
        client
            .dispatch(&support::repo(), &definition)
            .await
            .expect("acceptance");
        assert_dispatch(&fake.requests());
    }
}

#[tokio::test]
async fn dispatch_rejection_is_scrubbed_and_never_retried() {
    let definition: Definition = serde_json::from_value(support::definition()).expect("definition");
    let (client, fake) = Fake::client(vec![support::response(
        403,
        &json!({"message": support::TOKEN}),
    )]);
    let error = client
        .dispatch(&support::repo(), &definition)
        .await
        .expect_err("rejection");
    assert_eq!(
        (
            error.is_definite_rejection(),
            error.to_string().contains(support::TOKEN),
            fake.requests().len()
        ),
        (true, false, 1)
    );
}

#[tokio::test]
async fn dispatch_timeout_stays_ambiguous() {
    let definition: Definition = serde_json::from_value(support::definition()).expect("definition");
    let (client, fake) = Fake::client(vec![Err(Error::Transport(format!(
        "timeout {}",
        support::TOKEN
    )))]);
    let error = client
        .dispatch(&support::repo(), &definition)
        .await
        .expect_err("timeout");
    assert_eq!(
        (
            error.is_definite_rejection(),
            error.to_string().contains(support::TOKEN),
            fake.requests().len()
        ),
        (false, false, 1)
    );
}

#[test]
fn repository_scope_rejects_unproven_hosts_and_ambiguous_paths() {
    let invalid = [
        "https://ghe.example/o/r",
        "https://user@github.com/o/r",
        "o/r/extra",
        "o/../r",
        "https://github.com/o/r?token=x",
    ];
    for value in invalid {
        assert!(RepositoryRef::parse(value).is_err(), "{value}");
    }
}

#[test]
fn enterprise_forge_does_not_silently_dispatch_to_dotcom() {
    let forge = GitHubForge::new(Secret::new(support::TOKEN))
        .with_base_url("https://ghe.example/api/v3".to_owned());
    assert!(matches!(forge.cloud(), Err(Error::Unsupported(_))));
}
