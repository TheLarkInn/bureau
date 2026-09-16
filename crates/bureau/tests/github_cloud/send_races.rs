use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use bureau::forge::github::cloud::{self, Client, Response, Transport};
use bureau::github_cloud::{
    Control, Dispatch, Error, LEASE_ASSIGNMENT, dispatch, lease_key, read_state,
};
use bureau::process::Secret;
use rusqlite::Connection;

use super::control_support::{Fixture, accepted_replies, posts};
use super::support::{self, Fake};

fn replace_owner(database: &Path) {
    let connection = Connection::open(database).expect("competing connection");
    let changed = connection
        .execute(
            "UPDATE leases SET owner_id = 'replacement' WHERE assignment = ?1 AND external_id = ?2",
            (LEASE_ASSIGNMENT, lease_key("example/project", "receipt")),
        )
        .expect("HTTP is outside the ownership transaction");
    assert_eq!(changed, 1);
}

struct LostOwner {
    inner: Arc<Fake>,
    database: PathBuf,
}

#[async_trait]
impl Transport for LostOwner {
    async fn send(&self, request: reqwest::Request) -> Result<Response, cloud::Error> {
        let submission = request.method() == reqwest::Method::POST;
        let response = self.inner.send(request).await;
        if submission {
            replace_owner(&self.database);
        }
        response
    }
}

fn losing_client(fixture: &Fixture) -> Client {
    let transport = LostOwner {
        inner: fixture.fake.clone(),
        database: fixture.directory().join("state.db"),
    };
    Client::with_transport(Secret::new(support::TOKEN), Arc::new(transport))
}

#[tokio::test]
async fn an_accepted_post_cannot_record_success_after_ownership_changes() {
    let fixture = Fixture::new(accepted_replies()).await;
    let client = losing_client(&fixture);
    let control = Control {
        client: &client,
        ..fixture.control()
    };
    let result = dispatch(&control, "receipt", &support::automation()).await;
    let state = read_state(&fixture.root, "receipt").expect("durable intent");
    assert!(
        matches!(result, Err(Error::UncertainRecord { .. }))
            && state.dispatch == Dispatch::Prepared
    );
    assert_eq!(posts(&fixture.fake.requests()), 1);
}

#[tokio::test]
async fn failed_operation_cleanup_does_not_release_the_replacement_owner() {
    let fixture = Fixture::new(accepted_replies()).await;
    let client = losing_client(&fixture);
    let control = Control {
        client: &client,
        ..fixture.control()
    };
    let result = dispatch(&control, "receipt", &support::automation()).await;
    let leases = fixture.store.active(LEASE_ASSIGNMENT).expect("leases");
    assert!(result.is_err() && leases.len() == 1 && leases[0].owner_id == "replacement");
}

#[tokio::test]
async fn unusable_record_storage_prevents_submission() {
    let fixture = Fixture::new(vec![]).await;
    std::fs::write(&fixture.root, "not a directory").expect("storage fault");
    let result = dispatch(&fixture.control(), "receipt", &support::automation()).await;
    assert!(result.is_err() && posts(&fixture.fake.requests()) == 0);
}
