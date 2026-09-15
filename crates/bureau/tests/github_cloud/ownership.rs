use std::sync::Arc;
use std::time::Duration;

use bureau::github_cloud::{Control, Error, LEASE_ASSIGNMENT, dispatch, lease_key};
use bureau::state::{LeaseOwner, Store};

use super::control_support::Fixture;
use super::support::{self, Fake};

#[tokio::test]
async fn another_connection_cannot_submit_under_a_live_claim() {
    let fixture = Fixture::new(vec![]).await;
    let store =
        Arc::new(Store::open(&fixture.directory().join("state.db")).expect("second connection"));
    let key = lease_key("example/project", "receipt");
    let other =
        LeaseOwner::new(store, LEASE_ASSIGNMENT, "github_cloud", &key, "receipt").expect("owner");
    other.claim(Duration::from_secs(90)).expect("claim");
    let result = dispatch(&fixture.control(), "receipt", &support::automation()).await;
    assert!(matches!(result, Err(Error::Busy(_))) && fixture.fake.requests().len() == 1);
}

#[tokio::test]
async fn a_different_client_cannot_reuse_a_verified_principal() {
    let fixture = Fixture::new(vec![]).await;
    let (client, other) = Fake::with_token(vec![], "different-synthetic-secret");
    let control = Control {
        client: &client,
        ..fixture.control()
    };
    let result = dispatch(&control, "receipt", &support::automation()).await;
    assert!(matches!(result, Err(Error::Selection(_))) && other.requests().is_empty());
}

#[tokio::test]
async fn unsafe_local_keys_never_reach_the_transport() {
    let fixture = Fixture::new(vec![]).await;
    for key in ["", "..", "../outside", "a/b", "a\\b"] {
        let result = dispatch(&fixture.control(), key, &support::automation()).await;
        assert!(result.is_err());
    }
    assert_eq!(fixture.fake.requests().len(), 1);
}
