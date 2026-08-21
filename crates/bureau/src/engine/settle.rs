//! Settling a run: the escalate comment, the final events, and log
//! teardown.

use std::sync::Arc;

use super::context::{self, RunCtx};
use super::{RunOutcome, stream};
use crate::contract::StepOutcome;
use crate::forge::Pr;
use crate::runlog::{self, EventKind, RunTerminal};

const EXTERNAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) type RawResult = (StepOutcome, String, Option<Pr>);

pub(super) struct TerminalResult {
    terminal: RunTerminal,
    outcome: StepOutcome,
    message: String,
    pr: Option<Pr>,
}

impl TerminalResult {
    fn new(terminal: RunTerminal, raw: RawResult) -> Self {
        Self {
            terminal,
            outcome: raw.0,
            message: raw.1,
            pr: raw.2,
        }
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

fn configured_labels(labels: &[&String]) -> Vec<String> {
    labels
        .iter()
        .filter(|label| !label.trim().is_empty())
        .map(|label| (*label).clone())
        .collect()
}

fn label_changes(ctx: &RunCtx, terminal: RunTerminal) -> (Vec<String>, Vec<String>) {
    let work = &ctx.plan.assignment.work;
    match terminal {
        RunTerminal::Done => (
            Vec::new(),
            configured_labels(&[&work.abort_label, &work.escalate_label]),
        ),
        RunTerminal::Abort => (
            configured_labels(&[&work.abort_label]),
            configured_labels(&[&work.escalate_label]),
        ),
        RunTerminal::Escalate => (
            configured_labels(&[&work.escalate_label]),
            configured_labels(&[&work.abort_label]),
        ),
    }
}

async fn label_failure(ctx: &RunCtx, terminal: RunTerminal) -> Option<String> {
    let (add, remove) = label_changes(ctx, terminal);
    let update = ctx
        .plan
        .forge
        .update_labels(&ctx.plan.item.external_id, &add, &remove);
    let result = tokio::time::timeout(EXTERNAL_TIMEOUT, update).await;
    match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(format!("label update failed: {error}")),
        Err(_) => Some("label update timed out".to_owned()),
    }
}

fn escalation_comment(ctx: &RunCtx, message: &str) -> String {
    format!(
        "## Bureau needs human attention\n\n\
         **Run:** `{}`\n\
         **Assignment:** `{}`\n\
         **Reason:** {message}\n\n\
         Inspect: `bureau show {}`\n\
         Retry: `bureau retry {}`",
        ctx.plan.run_id, ctx.plan.assignment.name, ctx.plan.run_id, ctx.plan.run_id
    )
}

async fn comment_failure(ctx: &RunCtx, message: &str) -> Option<String> {
    let comment = escalation_comment(ctx, message);
    let post = ctx.plan.forge.comment(&ctx.plan.item.external_id, &comment);
    match tokio::time::timeout(EXTERNAL_TIMEOUT, post).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(format!("comment failed: {error}")),
        Err(_) => Some("comment timed out".to_owned()),
    }
}

fn append_failures(message: String, failures: [Option<String>; 2]) -> String {
    let failures: Vec<String> = failures.into_iter().flatten().collect();
    if failures.is_empty() {
        message
    } else {
        format!("{message} ({})", failures.join("; "))
    }
}

async fn terminal_comment(ctx: &RunCtx, terminal: RunTerminal, message: &str) -> Option<String> {
    if terminal == RunTerminal::Escalate {
        comment_failure(ctx, message).await
    } else {
        None
    }
}

pub(super) async fn project(ctx: &RunCtx, terminal: RunTerminal, raw: RawResult) -> TerminalResult {
    if let Some(reason) = context::ownership_reason(ctx) {
        return TerminalResult::new(RunTerminal::Abort, (StepOutcome::Failure, reason, None));
    }
    let label = label_failure(ctx, terminal).await;
    let comment = terminal_comment(ctx, terminal, &raw.1).await;
    TerminalResult::new(
        terminal,
        (raw.0, append_failures(raw.1, [label, comment]), raw.2),
    )
}

fn close(ctx: RunCtx) -> RunOutcome {
    let (run_id, cost_usd) = (ctx.plan.run_id.clone(), ctx.cost_usd);
    teardown(ctx.log);
    RunOutcome {
        run_id,
        outcome: StepOutcome::NoWork,
        cost_usd,
        message: String::new(),
        pr: None,
    }
}

fn recorded_terminal(ctx: &RunCtx) -> std::io::Result<Option<runlog::RunFinishedData>> {
    let directory = stream::lock(&ctx.log).dir().to_path_buf();
    Ok(runlog::replay_state(&directory)?.finished)
}

fn close_recorded(ctx: RunCtx, data: runlog::RunFinishedData) -> RunOutcome {
    let run_id = ctx.plan.run_id.clone();
    teardown(ctx.log);
    RunOutcome::finished(&run_id, data)
}

fn close_replay_error(ctx: RunCtx, error: &std::io::Error) -> RunOutcome {
    let run_id = ctx.plan.run_id.clone();
    teardown(ctx.log);
    RunOutcome::bare(
        &run_id,
        StepOutcome::Failure,
        format!("checking terminal state failed: {error}"),
    )
}

fn append_finished(ctx: &RunCtx, result: &TerminalResult) {
    context::append(
        ctx,
        EventKind::Output,
        runlog::output(None, "run", &result.message),
    );
    let disposition = runlog::TerminalDisposition::for_outcome(result.outcome, result.pr.is_some());
    let finished = runlog::run_finished_full(
        Some(result.terminal),
        result.outcome,
        &result.message,
        ctx.cost_usd,
        result.pr.as_ref(),
        disposition,
    );
    context::append(ctx, EventKind::RunFinished, finished);
}

/// Appends the run's message and `run_finished`, then tears the log
/// down. The worktree guard must already have dropped.
pub(super) fn finish(ctx: RunCtx, result: TerminalResult) -> RunOutcome {
    let _terminal = runlog::lock_terminal_append();
    match recorded_terminal(&ctx) {
        Ok(Some(data)) => return close_recorded(ctx, data),
        Err(error) => return close_replay_error(ctx, &error),
        Ok(None) => {}
    }
    append_finished(&ctx, &result);
    let mut settled = close(ctx);
    settled.outcome = result.outcome;
    settled.message = result.message;
    settled.pr = result.pr;
    settled
}

/// A paused run exits unfinished, without a terminal event: the log
/// closes and re-entry resumes the run from its events.
pub(super) fn paused(ctx: RunCtx) -> RunOutcome {
    let message = "run paused at a step boundary; remove the PAUSE marker and resume".to_owned();
    context::append(
        &ctx,
        EventKind::Output,
        runlog::output(None, "run", &message),
    );
    let mut settled = close(ctx);
    settled.message = message;
    settled
}
