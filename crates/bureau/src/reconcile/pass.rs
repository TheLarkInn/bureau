//! One reconcile pass's products — what it started, what it skipped —
//! and its pacing: a jittered sleep between passes.

use std::time::Duration;

use tokio::task::JoinHandle;

use super::Error;
use crate::engine::RunOutcome;

/// A claimed, started run.
pub struct Started {
    /// The run id.
    pub run_id: String,
    /// The run's task; joining it yields the outcome.
    pub handle: JoinHandle<RunOutcome>,
}

/// What one pass did. Library code never prints: per-assignment
/// failures are data here, for the caller to report or drop.
pub struct PassReport {
    /// Runs claimed and started this pass.
    pub started: Vec<Started>,
    /// `(assignment, error)` for each assignment the pass skipped; the
    /// level-triggered loop retries it next pass.
    pub failed: Vec<(String, Error)>,
}

/// The interval ± 25%, derived from the clock's low bits. The injected
/// clock guarantees only millis, but those still decorrelate daemons
/// started milliseconds apart.
fn jittered(interval: Duration, now_ms: u64) -> Duration {
    let base = interval.as_nanos();
    let spread = (base / 4).max(1);
    let shifted = base.saturating_sub(spread) + u128::from(now_ms) % (2 * spread + 1);
    Duration::from_nanos(u64::try_from(shifted).unwrap_or(u64::MAX))
}

/// Sleeps a jittered interval, returning early on a wake; a closed channel degrades to plain sleeps.
pub(super) async fn wait(
    interval: Duration,
    wake: &mut tokio::sync::mpsc::Receiver<()>,
    now_ms: u64,
) {
    let Ok(woken) = tokio::time::timeout(jittered(interval, now_ms), wake.recv()).await else {
        return; // the jittered interval elapsed
    };
    if woken.is_none() {
        tokio::time::sleep(interval).await;
    }
}

/// The pass result: the report, or the first failure when the pass
/// started nothing at all.
pub(super) fn settle(
    failed: Vec<(String, Error)>,
    started: Vec<Started>,
) -> Result<PassReport, Error> {
    let mut failed = failed.into_iter();
    if started.is_empty() {
        if let Some((_, first)) = failed.next() {
            return Err(first);
        }
    }
    Ok(PassReport {
        started,
        failed: failed.collect(),
    })
}
