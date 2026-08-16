//! Step requests, trust gates, deterministic commands, and agent adapters.

use std::collections::BTreeMap;
use std::time::Duration;

use super::machine::{RunCtx, WtCtx};
use super::stream::{self, LogSink};
use super::{artifact, deadline, plugins};
use crate::adapters::{self, Execution, Usage};
use crate::config::{StepDef, StepKind};
use crate::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust};
use crate::process::{SpawnRequest, SpawnResult, shared_log, spawn};

/// Default per-step timeout, matching the `fake` adapter.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Runs one code step and returns its result.
pub(super) async fn execute(
    ctx: &RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
) -> Execution {
    let timeout = deadline::bounded(
        step.timeout_secs,
        Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        ctx.remaining(),
    );
    match step.kind {
        StepKind::Deterministic => deterministic(ctx, wt, step, request, timeout).await,
        StepKind::Agent => agent(ctx, step, request, agent_timeout(ctx, step)).await,
        StepKind::Decision | StepKind::Concurrent => {
            failed_step("routing and concurrent steps do not run through this path")
        }
    }
}

/// A deterministic step: `sh -c <run>` in the worktree under the
/// layer-0 contract. It receives NO credentials in its environment —
/// DESIGN.md section 10: a step that does not need to push never
/// receives a token that can push, and deterministic steps run repo
/// code and build tooling. The resolved credentials still form the
/// spawn's scrub list, so any of them the step manages to print is
/// redacted from the log. The engine's own mirror and push paths
/// resolve their credentials straight from the plan, never via a
/// step's environment.
async fn deterministic(
    ctx: &RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
) -> Execution {
    let spawned = spawn(SpawnRequest {
        argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            step.run.clone().unwrap_or_default(),
        ],
        dir: wt.worktree.path().to_path_buf(),
        env: BTreeMap::new(),
        stdin: request.to_json().unwrap_or_default(),
        timeout,
        secrets: ctx.secrets(),
        log: Some(shared_log(LogSink::new(
            &step.name,
            &ctx.log,
            ctx.plan.lease.clone(),
        ))),
        cancel: adapters::cancel_path(request),
    })
    .await;
    Execution::new(
        derive_result(request.trust, &spawned),
        Usage::zero("deterministic"),
    )
}

/// Derives the step result: a contract document on stdout wins;
/// otherwise exit 0 is `Success` with the trimmed stdout as the lone
/// output. Deterministic steps preserve their input trust either way.
fn derive_result(trust: Trust, spawned: &SpawnResult) -> StepResult {
    let parsed = StepResult::from_json(&spawned.stdout).is_ok();
    let mut result = adapters::result_from_spawn(spawned);
    result.trust = trust;
    if !parsed && result.outcome == StepOutcome::Success {
        let stdout = String::from_utf8_lossy(&spawned.stdout).trim().to_owned();
        result
            .outputs
            .insert("stdout".to_owned(), serde_json::Value::String(stdout));
    }
    result
}

/// Runs the role adapter and grades its output `Derived`.
async fn agent(
    ctx: &RunCtx,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
) -> Execution {
    let prepared = match prepare_agent(ctx, step, request) {
        Ok(prepared) => prepared,
        Err(message) => {
            return blocked_step(&message).halt();
        }
    };
    let sink = shared_log(LogSink::new(&step.name, &ctx.log, ctx.plan.lease.clone()));
    let execution = adapters::execute(
        &prepared.0,
        step,
        request,
        timeout,
        ctx.secrets(),
        Some(sink),
    )
    .await;
    finish_agent(ctx, step, request, prepared.1, execution).await
}

fn prepare_agent(
    ctx: &RunCtx,
    step: &StepDef,
    request: &StepRequest,
) -> Result<(crate::config::Role, Option<plugins::ActiveAgent>), String> {
    let role =
        role_for(ctx, step).ok_or_else(|| format!("step `{}` names an unknown role", step.name))?;
    let run_dir = stream::lock(&ctx.log).dir().to_path_buf();
    let activation = plugins::activate(&ctx.plan, role, &run_dir, &request.worktree)?;
    let mut runtime_role = role.clone();
    if let Some(active) = activation.as_ref() {
        active.agent_name().clone_into(&mut runtime_role.agent);
    }
    Ok((runtime_role, activation))
}

async fn finish_agent(
    ctx: &RunCtx,
    step: &StepDef,
    request: &StepRequest,
    activation: Option<plugins::ActiveAgent>,
    mut execution: Execution,
) -> Execution {
    execution.result.trust = Trust::Derived;
    if let Err(message) = plugins::restore(activation) {
        execution.result = blocked_result(&message);
        return execution.halt();
    }
    enforce_measured_cost(ctx, &mut execution);
    publish_artifacts(ctx, step, request, &mut execution).await;
    execution
}

fn agent_timeout(ctx: &RunCtx, step: &StepDef) -> Duration {
    deadline::bounded(step.timeout_secs, ctx.remaining(), ctx.remaining())
}

fn role_for<'a>(ctx: &'a RunCtx, step: &StepDef) -> Option<&'a crate::config::Role> {
    step.role
        .as_deref()
        .and_then(|name| ctx.plan.roles.get(name))
}

async fn publish_artifacts(
    ctx: &RunCtx,
    step: &StepDef,
    request: &StepRequest,
    execution: &mut Execution,
) {
    let destination = stream::lock(&ctx.log)
        .dir()
        .join("artifacts")
        .join(&step.name);
    if let Err(message) = artifact::materialize(
        &mut execution.result.artifacts,
        &request.worktree,
        &destination,
        &ctx.secrets(),
    )
    .await
    {
        execution.result = failure_result(&format!("publishing artifacts failed: {message}"));
    }
}

fn enforce_measured_cost(ctx: &RunCtx, execution: &mut Execution) {
    let capped = ctx.plan.assignment.limits.max_cost_per_day_usd.is_some();
    if capped && execution.usage.cost_usd.is_none() {
        execution.result = failure_result(
            "this assignment has `max_cost_per_day_usd`, but the adapter did not report measurable usage; no further cost-bearing step may run",
        );
    }
}

/// A synthetic failure for a step that cannot run at all.
fn failed_step(message: &str) -> Execution {
    Execution::new(failure_result(message), Usage::zero("engine"))
}

fn blocked_step(message: &str) -> Execution {
    Execution::new(blocked_result(message), Usage::zero("engine"))
}

fn failure_result(message: &str) -> StepResult {
    synthetic_result(StepOutcome::Failure, message)
}

fn blocked_result(message: &str) -> StepResult {
    synthetic_result(StepOutcome::Blocked, message)
}

fn synthetic_result(outcome: StepOutcome, message: &str) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    }
}
