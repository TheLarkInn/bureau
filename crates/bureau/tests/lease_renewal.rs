//! Active runs renew ownership and stop when renewal is lost.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bureau::state::{Store, maintain_lease};

const ASSIGNMENT: &str = "assignment";
const ITEM: &str = "42";

#[tokio::test]
async fn active_run_renews_past_the_original_expiry() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let ttl = Duration::from_millis(90);
    assert!(
        store
            .try_claim(ASSIGNMENT, "github", ITEM, ttl)
            .expect("claim")
    );
    let marker = marker("renew");
    maintain_lease(
        store.clone(),
        ASSIGNMENT,
        ITEM,
        ttl,
        &marker,
        tokio::time::sleep(Duration::from_millis(240)),
    )
    .await;
    assert_eq!(store.active(ASSIGNMENT).expect("active").len(), 1);
}

#[tokio::test]
async fn lost_lease_writes_an_actionable_cancel_reason() {
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let ttl = Duration::from_millis(120);
    assert!(
        store
            .try_claim(ASSIGNMENT, "github", ITEM, ttl)
            .expect("claim")
    );
    let marker = marker("lost");
    let future = wait_for_marker(marker.clone());
    let released = release_soon(store.clone());
    let ((), ()) = tokio::join!(
        maintain_lease(store, ASSIGNMENT, ITEM, ttl, &marker, future),
        released
    );
    let reason = std::fs::read_to_string(&marker).expect("reason");
    std::fs::remove_file(marker).expect("cleanup");
    assert!(reason.contains("lease renewal failed") && reason.contains(ITEM));
}

async fn release_soon(store: Arc<Store>) {
    tokio::time::sleep(Duration::from_millis(60)).await;
    store.release(ASSIGNMENT, ITEM).expect("release");
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
