//! Lease renewal while one run future remains active.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::Store;

pub async fn maintain<T>(
    store: Arc<Store>,
    assignment: &str,
    external_id: &str,
    ttl: Duration,
    cancel: &Path,
    future: impl Future<Output = T>,
) -> T {
    let interval = (ttl / 3).max(Duration::from_millis(10));
    let mut future = Box::pin(future);
    if let Some(output) = renewal_loop(
        &store,
        assignment,
        external_id,
        ttl,
        interval,
        future.as_mut(),
    )
    .await
    {
        return output;
    }
    cancel_for_lost_lease(cancel, assignment, external_id);
    future.await
}

async fn renewal_loop<T, F>(
    store: &Store,
    assignment: &str,
    external_id: &str,
    ttl: Duration,
    interval: Duration,
    mut future: Pin<&mut F>,
) -> Option<T>
where
    F: Future<Output = T>,
{
    loop {
        if let Some(output) = tick(future.as_mut(), interval).await {
            return Some(output);
        }
        if !matches!(store.renew(assignment, external_id, ttl), Ok(true)) {
            return None;
        }
    }
}

async fn tick<T, F>(future: Pin<&mut F>, interval: Duration) -> Option<T>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(interval, future).await.ok()
}

fn cancel_for_lost_lease(path: &Path, assignment: &str, external_id: &str) {
    let reason = format!(
        "lease renewal failed for assignment `{assignment}` item `{external_id}`; this run stopped before continuing without ownership"
    );
    let _ = std::fs::write(path, reason);
}
