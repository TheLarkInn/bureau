//! One native factory belongs to one unfinished Bureau step and retained worktree.

mod authentication;
mod catalog;
mod connection;
pub mod context;
mod credentials;
mod invoke;
mod journal;
mod launch;
mod lifecycle;
mod observer;
mod policy;
mod prepare;
mod race;
pub(super) mod recovery;
mod resources;
mod result;
mod runtime;
mod session;
mod supervised;
mod tools;

use std::time::Duration;

use crate::adapters::Execution;
use crate::config::StepDef;
use crate::contract::StepRequest;

use super::context::{RunCtx, WtCtx};
use journal::Journal;

pub(super) fn secrets(plan: &super::RunPlan) -> Vec<crate::process::Secret> {
    journal::secrets(plan)
}

pub(super) fn preserve(ctx: &RunCtx, message: &str) {
    if let Ok(journal) = Journal::new(ctx) {
        let _ = journal.pause(message);
    }
}

pub(super) fn synchronize_cost(ctx: &mut RunCtx) {
    if let Ok(records) = recovery::records(ctx) {
        ctx.factory_cost(&records);
    }
}

async fn prepared(
    ctx: &RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
    journal: &Journal,
) -> Result<Execution, String> {
    let prepared = prepare::prepare(ctx, wt, step, request, journal)?;
    runtime::execute(ctx, step, &prepared, timeout, journal).await?;
    result::execution(
        &journal.record(&prepared.intent.session_id)?,
        &ctx.secrets(),
    )
}

async fn run(
    ctx: &RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
) -> Execution {
    let journal = match Journal::new(ctx) {
        Ok(journal) => journal,
        Err(error) => return result::halt(ctx, &error),
    };
    match Box::pin(prepared(ctx, wt, step, request, timeout, &journal)).await {
        Ok(execution) => execution,
        Err(error) => result::halt(ctx, &error),
    }
}

type ExecutionFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Execution> + Send + 'a>>;

pub(super) fn execute<'a>(
    ctx: &'a RunCtx,
    wt: &'a WtCtx,
    step: &'a StepDef,
    request: &'a StepRequest,
    timeout: Duration,
) -> ExecutionFuture<'a> {
    Box::pin(run(ctx, wt, step, request, timeout))
}
