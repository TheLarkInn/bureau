//! Run cancellation and deadline messages.

use super::deadline;
use super::machine::RunCtx;

pub(super) fn cancel_reason(ctx: &RunCtx) -> Option<String> {
    let path = ctx.cancel_path();
    if !path.exists() {
        return None;
    }
    let reason = std::fs::read_to_string(path).unwrap_or_default();
    let reason = reason.trim();
    Some(if reason.is_empty() {
        "cancelled".to_owned()
    } else {
        reason.to_owned()
    })
}

pub(super) fn deadline_message(ctx: &RunCtx) -> String {
    let hours = ctx
        .plan
        .assignment
        .limits
        .max_run_hours
        .unwrap_or(deadline::DEFAULT_RUN_HOURS);
    format!(
        "run exceeded its {hours} hour deadline; inspect the preserved evidence, then run `bureau retry {}`",
        ctx.plan.run_id
    )
}
