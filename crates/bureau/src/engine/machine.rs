//! The state machine: step entry, the gates, and the CANCEL/PAUSE checks.

use super::context::{self, RunCtx, WtCtx};
use super::{RunPlan, checkpoint, execute};
use crate::adapters::Execution;
use crate::config::{Repo, StepDef};
use crate::contract::StepRequest;
use crate::runlog::{self, EventKind};

mod route;

/// Why the machine stopped.
pub(super) enum Stop {
    /// Reached the `done` terminal.
    Done,
    /// `abort`, a fail-closed route, or the CANCEL marker.
    Fail(String),
    /// `escalate`: comment for a human, outcome Blocked.
    Escalate(String),
    /// The PAUSE marker appeared at a step boundary; the run exits
    /// unfinished and resumes where it left off.
    Pause,
}

fn started_data(ctx: &RunCtx, worktree: &std::path::Path, step: &StepDef) -> serde_json::Value {
    let Some((name, role)) = step
        .role
        .as_deref()
        .and_then(|name| ctx.plan.roles.get(name).map(|role| (name, role)))
    else {
        return runlog::step_started(&step.name);
    };
    runlog::step_started_agent(
        &step.name,
        name,
        &role.agent,
        &crate::adapters::resolved_agent(role, worktree),
    )
}

/// The machine loop: route, check CANCEL, run steps, stop at terminals.
pub(super) async fn run_loop(ctx: &mut RunCtx, wt: &WtCtx) -> Stop {
    let mut route = ctx.start.clone();
    loop {
        match route::advance(ctx, wt, route).await {
            route::Turn::Next(next) => route = next,
            route::Turn::Stop(stop) => return stop,
        }
    }
}

/// Executes a step between its started and finished events.
pub(super) async fn run_step(
    ctx: &mut RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
) -> Execution {
    context::append(
        ctx,
        EventKind::StepStarted,
        started_data(ctx, &request.worktree, step),
    );
    ctx.begin_attempt(&step.name);
    let mut result = execute::execute(ctx, wt, step, request).await;
    if result.is_halted() || context::ownership_reason(ctx).is_some() {
        return result.halt();
    }
    checkpoint::save_result(ctx, wt, step, &mut result).await;
    let finished = runlog::step_finished_full(&step.name, &result);
    context::append(ctx, EventKind::StepFinished, finished);
    result
}

/// The assignment's primary repo, by registry name.
pub(super) fn primary_repo(plan: &RunPlan) -> Result<(&str, &Repo), String> {
    let name = plan
        .assignment
        .primary_repo()
        .ok_or_else(|| "assignment lists no repos".to_owned())?;
    let repo = plan
        .repos
        .get(name)
        .ok_or_else(|| format!("primary repo `{name}` is not in the registry"))?;
    Ok((name, repo))
}

#[cfg(test)]
mod tests;
