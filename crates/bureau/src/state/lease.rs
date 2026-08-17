//! Lease renewal while one run future remains active.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use super::LeaseOwner;

async fn tick<T, F>(future: Pin<&mut F>, interval: Duration) -> Option<T>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(interval, future).await.ok()
}

async fn renewal_loop<T, F>(
    owner: &LeaseOwner,
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
        if !matches!(owner.renew(ttl), Ok(true)) {
            return None;
        }
    }
}

/// Records the lease loss in the run's `CANCEL` marker. The marker is the
/// only channel this layer has; a failed write has nowhere further to go
/// (the same treatment as the member-cancel mirror in the engine), so the
/// caller drops it.
fn cancel_for_lost_lease(path: &Path, owner: &LeaseOwner) -> std::io::Result<()> {
    let reason = format!(
        "lease renewal failed for assignment `{}` item `{}`; this run stopped before continuing without ownership",
        owner.assignment(),
        owner.external_id()
    );
    std::fs::write(path, reason)
}

pub async fn maintain_lease<T>(
    owner: LeaseOwner,
    ttl: Duration,
    cancel: &Path,
    future: impl Future<Output = T>,
) -> Option<T> {
    let interval = (ttl / 3).max(Duration::from_millis(10));
    let mut future = Box::pin(future);
    if let Some(output) = renewal_loop(&owner, ttl, interval, future.as_mut()).await {
        return Some(output);
    }
    let _ = cancel_for_lost_lease(cancel, &owner);
    None
}
