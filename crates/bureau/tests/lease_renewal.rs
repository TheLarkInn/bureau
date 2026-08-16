//! Active runs renew ownership and stop when renewal is lost.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bureau::state::{LeaseOwner, Store, maintain_lease};

const ASSIGNMENT: &str = "assignment";
const ITEM: &str = "42";

#[test]
fn live_lease_rejects_a_different_run_owner() {
    let store = Store::open_in_memory().expect("store");
    let ttl = Duration::from_secs(60);
    let claimed = store
        .try_claim_run(ASSIGNMENT, "github", ITEM, "run-1", ttl)
        .expect("claim");
    let resumed = store
        .resume_claim(ASSIGNMENT, "github", ITEM, "run-2", ttl)
        .expect("resume");
    store
        .release_run(ASSIGNMENT, ITEM, "run-2")
        .expect("wrong-owner release is idempotent");
    let owner = store.active(ASSIGNMENT).expect("active")[0].run_id.clone();
    assert_eq!((claimed, resumed, owner), (true, false, "run-1".to_owned()));
}

#[test]
fn same_run_cannot_resume_a_live_supervisor_generation() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let ttl = Duration::from_secs(60);
    let first = owner(store.clone(), "run-1");
    let second = owner(store, "run-1");
    first.claim(ttl).expect("claim");
    let resumed = second.claim(ttl).expect("resume");
    assert!(!resumed);
}

#[test]
fn sequential_generations_use_distinct_random_tokens() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let first = owner(store.clone(), "run-1");
    first.claim(Duration::from_secs(60)).expect("first claim");
    let first_id = store.active(ASSIGNMENT).expect("active")[0]
        .owner_id
        .clone();
    first.release().expect("release");
    let second = owner(store.clone(), "run-1");
    second.claim(Duration::from_secs(60)).expect("second claim");
    let second_id = store.active(ASSIGNMENT).expect("active")[0]
        .owner_id
        .clone();
    assert!(first_id != second_id && first_id.len() == 32 && second_id.len() == 32);
}

#[tokio::test]
async fn active_run_renews_past_the_original_expiry() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let ttl = Duration::from_millis(90);
    let owner = owner(store.clone(), "run-1");
    assert!(owner.claim(ttl).expect("claim"));
    let marker = marker("renew");
    let maintained = maintain_lease(
        owner,
        ttl,
        &marker,
        tokio::time::sleep(Duration::from_millis(240)),
    )
    .await;
    assert_eq!(
        (maintained, store.active(ASSIGNMENT).expect("active").len()),
        (Some(()), 1)
    );
}

#[tokio::test]
async fn lost_lease_writes_an_actionable_cancel_reason() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let ttl = Duration::from_millis(120);
    let owner = owner(store, "run-lost");
    assert!(owner.claim(ttl).expect("claim"));
    let marker = marker("lost");
    let future = wait_for_marker(marker.clone());
    let released = release_soon(owner.clone());
    let (maintained, ()) = tokio::join!(maintain_lease(owner, ttl, &marker, future), released);
    let reason = std::fs::read_to_string(&marker).expect("reason");
    std::fs::remove_file(marker).expect("cleanup");
    assert!(
        maintained.is_none() && reason.contains("lease renewal failed") && reason.contains(ITEM)
    );
}

async fn release_soon(owner: LeaseOwner) {
    tokio::time::sleep(Duration::from_millis(60)).await;
    owner.release().expect("release");
}

fn owner(store: Arc<Store>, run_id: &str) -> LeaseOwner {
    LeaseOwner::new(store, ASSIGNMENT, "github", ITEM, run_id).expect("owner")
}

async fn wait_for_marker(path: PathBuf) {
    while !path.exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn marker(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("bureau-lease-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}
