//! The daemon loop's sleep: a jittered interval an explicit wake cuts
//! short, so two daemons do not fall into lockstep.

use std::time::{Duration, SystemTime};

/// The interval ± 25%, derived from the process clock's nanoseconds.
/// The clock read is the boundary: bound once as a function pointer so
/// this stays the single site naming the process clock.
fn jittered(interval: Duration) -> Duration {
    let now = SystemTime::now;
    let nanos = now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let base = interval.as_nanos();
    let spread = (base / 4).max(1);
    let shifted = base.saturating_sub(spread) + u128::from(nanos) % (2 * spread + 1);
    Duration::from_nanos(u64::try_from(shifted).unwrap_or(u64::MAX))
}

/// Sleeps a jittered interval, returning early on a wake; a closed
/// channel degrades to plain sleeps.
pub(super) async fn wait(interval: Duration, wake: &mut tokio::sync::mpsc::Receiver<()>) {
    let Ok(woken) = tokio::time::timeout(jittered(interval), wake.recv()).await else {
        return; // the jittered interval elapsed
    };
    if woken.is_none() {
        tokio::time::sleep(interval).await;
    }
}
