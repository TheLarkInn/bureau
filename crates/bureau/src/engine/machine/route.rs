//! Boundary checks and routing for one machine turn.

use crate::adapters::Execution;
use crate::config::{StepDef, StepKind};

use super::super::{approval, concurrent, control, edge, execute};
use super::{RunCtx, Stop, WtCtx, run_step};

pub(super) enum Turn {
    Next(edge::Route),
    Stop(Stop),
}

pub(super) async fn advance(ctx: &mut RunCtx, wt: &WtCtx, route: edge::Route) -> Turn {
    if let Some(stop) = boundary_stop(ctx).await {
        return Turn::Stop(stop);
    }
    route_turn(ctx, wt, route).await
}

async fn route_turn(ctx: &mut RunCtx, wt: &WtCtx, route: edge::Route) -> Turn {
    match route {
        edge::Route::Step(name) => step_turn(ctx, wt, &name).await,
        edge::Route::Done => Turn::Stop(Stop::Done),
        edge::Route::Fail(message) => Turn::Stop(Stop::Fail(message)),
        edge::Route::Escalate(message) => Turn::Stop(Stop::Escalate(message)),
    }
}

async fn boundary_stop(ctx: &RunCtx) -> Option<Stop> {
    if let Some(reason) = control::cancel_reason(ctx) {
        return Some(Stop::Fail(reason));
    }
    if ctx.remaining().is_zero() {
        return Some(Stop::Escalate(control::deadline_message(ctx)));
    }
    approval::check(ctx).await.err().map(Stop::Escalate)
}

async fn step_turn(ctx: &mut RunCtx, wt: &WtCtx, name: &str) -> Turn {
    let Some(step) = ctx
        .plan
        .pipeline
        .steps
        .iter()
        .find(|step| step.name == name)
        .cloned()
    else {
        return Turn::Stop(Stop::Fail(format!("unknown step `{name}`")));
    };
    match step.kind {
        StepKind::Decision => Turn::Next(decision_route(ctx, &step)),
        StepKind::Concurrent => concurrent_route(ctx, wt, &step).await,
        StepKind::Deterministic | StepKind::Agent => code_route(ctx, wt, &step).await,
    }
}

async fn concurrent_route(ctx: &mut RunCtx, wt: &WtCtx, group: &StepDef) -> Turn {
    let resuming = ctx
        .groups
        .get(&group.name)
        .is_some_and(|record| record.result.is_none());
    if !resuming && let Some(reason) = attempts_check(ctx, group) {
        return Turn::Stop(Stop::Escalate(reason));
    }
    let execution = concurrent::run(ctx, wt, group).await;
    route_execution(ctx, group, execution)
}

async fn code_route(ctx: &mut RunCtx, wt: &WtCtx, step: &StepDef) -> Turn {
    if let Some(reason) = attempts_check(ctx, step) {
        return Turn::Stop(Stop::Escalate(reason));
    }
    let request = execute::build_request(ctx, step, wt.worktree.path());
    if let Some(reason) = execute::trust_check(&ctx.plan, step, &request) {
        return Turn::Stop(Stop::Escalate(reason));
    }
    let execution = run_step(ctx, wt, step, &request).await;
    route_execution(ctx, step, execution)
}

fn route_execution(ctx: &mut RunCtx, step: &StepDef, execution: Execution) -> Turn {
    let outcome = execution.result.outcome;
    let detail = execution.result.message.clone();
    ctx.record(&step.name, execution);
    if let Some(reason) = control::cancel_reason(ctx) {
        return Turn::Stop(Stop::Fail(reason));
    }
    if ctx.remaining().is_zero() {
        return Turn::Stop(Stop::Escalate(control::deadline_message(ctx)));
    }
    Turn::Next(edge::route_after(
        step,
        outcome,
        Some(&detail),
        &ctx.plan.pipeline,
    ))
}

fn attempts_check(ctx: &RunCtx, step: &StepDef) -> Option<String> {
    let entries = ctx.attempts.get(&step.name).copied().unwrap_or(0);
    (entries >= step.max_attempts).then(|| format!("step `{}` exceeded max attempts", step.name))
}

fn decision_route(ctx: &RunCtx, step: &StepDef) -> edge::Route {
    let Some(over) = step.over.as_deref() else {
        return edge::Route::Fail(format!("decision `{}` names no `over` step", step.name));
    };
    let Some(outcome) = ctx.outcome_of(over) else {
        return edge::Route::Fail(format!(
            "decision `{}` has no recorded outcome for `{over}`",
            step.name
        ));
    };
    edge::route_decision(step, outcome, &ctx.plan.pipeline)
}
