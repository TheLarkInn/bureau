//! Settling a run: the escalate comment, the final events, and log
//! teardown.

use std::sync::Arc;

use super::machine::{self, RunCtx};
use super::{RunOutcome, control, stream};
use crate::contract::StepOutcome;
use crate::forge::Pr;
use crate::runlog::{self, EventKind};

const COMMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The `escalate` terminal: comment on the item (best effort), then
/// Blocked. A failed comment is noted in the message, never retried.
pub(super) async fn escalate(ctx: &RunCtx, message: String) -> (StepOutcome, String, Option<Pr>) {
    if let Some(reason) = control::ownership_reason(ctx) {
        return (StepOutcome::Failure, reason, None);
    }
    let comment = format!("run `{}`: {message}", ctx.plan.run_id);
    let result = tokio::time::timeout(
        COMMENT_TIMEOUT,
        ctx.plan.forge.comment(&ctx.plan.item.external_id, &comment),
    )
    .await;
    match result {
        Ok(Ok(())) => (StepOutcome::Blocked, message, None),
        Ok(Err(error)) => (
            StepOutcome::Blocked,
            format!("{message} (comment failed: {error})"),
            None,
        ),
        Err(_) => (
            StepOutcome::Blocked,
            format!("{message} (comment timed out)"),
            None,
        ),
    }
}

/// Appends the run's message and `run_finished`, then tears the log
/// down. The worktree guard must already have dropped.
pub(super) fn finish(
    ctx: RunCtx,
    outcome: StepOutcome,
    message: String,
    pr: Option<Pr>,
) -> RunOutcome {
    machine::append(
        &ctx,
        EventKind::Output,
        runlog::output(None, "run", &message),
    );
    let disposition = runlog::TerminalDisposition::for_outcome(outcome, pr.is_some());
    let finished =
        runlog::run_finished_full(outcome, &message, ctx.cost_usd, pr.as_ref(), disposition);
    machine::append(&ctx, EventKind::RunFinished, finished);
    let (run_id, cost_usd) = (ctx.plan.run_id.clone(), ctx.cost_usd);
    teardown(ctx.log);
    RunOutcome {
        run_id,
        outcome,
        cost_usd,
        message,
        pr,
    }
}

/// Closes the log and rewrites the derived state cache. Best effort:
/// the event log is already the source of truth.
fn teardown(log: stream::Shared) {
    let Ok(mutex) = Arc::try_unwrap(log) else {
        return;
    };
    let appender = mutex
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = appender.dir().to_path_buf();
    let _ = appender.close();
    if let Ok(state) = runlog::replay_state(&dir) {
        let _ = runlog::write_state_cache(&dir, &state);
    }
}
