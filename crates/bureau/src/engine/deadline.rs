//! Complete-run deadline derived from the original run event.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default emergency deadline when an assignment omits one.
pub(super) const DEFAULT_RUN_HOURS: u64 = 24;

pub(super) fn at(started_at_ms: u64, configured_hours: Option<u64>) -> tokio::time::Instant {
    let total = Duration::from_secs(
        configured_hours
            .unwrap_or(DEFAULT_RUN_HOURS)
            .saturating_mul(3600),
    );
    let elapsed = if started_at_ms == 0 {
        Duration::ZERO
    } else {
        Duration::from_millis(now_ms().saturating_sub(started_at_ms))
    };
    tokio::time::Instant::now() + total.saturating_sub(elapsed)
}

pub(super) fn remaining(deadline: tokio::time::Instant) -> Duration {
    deadline.saturating_duration_since(tokio::time::Instant::now())
}

pub(super) fn bounded(
    configured_secs: Option<u64>,
    fallback: Duration,
    remaining: Duration,
) -> Duration {
    let configured = configured_secs.map_or(fallback, Duration::from_secs);
    configured.min(remaining)
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(millis).unwrap_or(u64::MAX)
}
