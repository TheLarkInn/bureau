//! Optional assignment-limit headroom.

use crate::config::Limits;

/// Remaining run slots: the minimum across every configured limit.
pub(super) fn remaining(
    limits: &Limits,
    open_prs: usize,
    live: u32,
    hour: u32,
    day: u32,
    spent: f64,
) -> usize {
    let slots = [
        count(limits.max_concurrent, live),
        count(limits.max_runs_per_hour, hour),
        count(limits.max_runs_per_day, day),
        count(
            limits.max_open_prs,
            u32::try_from(open_prs).unwrap_or(u32::MAX),
        ),
    ];
    let min = slots.into_iter().min().unwrap_or(usize::MAX);
    cost(spent, limits.max_cost_per_day_usd).min(min)
}

fn count(limit: Option<u32>, used: u32) -> usize {
    limit.map_or(usize::MAX, |max| {
        usize::try_from(max.saturating_sub(used)).unwrap_or(0)
    })
}

fn cost(spent: f64, max_usd: Option<f64>) -> usize {
    if max_usd.is_some_and(|max| spent >= max) {
        0
    } else {
        usize::MAX
    }
}
