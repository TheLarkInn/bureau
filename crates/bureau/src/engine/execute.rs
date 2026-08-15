//! Running one step: request assembly, the trust check, and the two
//! code-running kinds (DESIGN.md layer 4 and section 9).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::RunPlan;
use super::machine::{RunCtx, WtCtx};
use super::stream::{self, LogSink};
use super::{artifact, deadline};
use crate::adapters::{self, Execution, Usage};
use crate::config::{StepDef, StepKind};
use crate::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust};
use crate::process::{SpawnRequest, SpawnResult, shared_log, spawn};

/// Default per-step timeout when the pipeline sets none, matching the
/// `fake` adapter's.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Assembles a step's request: only `inputs_from` steps contribute
/// inputs and artifacts — no ambient accumulation.
pub(super) fn build_request(ctx: &RunCtx, step: &StepDef, worktree: &Path) -> StepRequest {
    StepRequest {
        schema: SCHEMA_VERSION.to_owned(),
        run_id: ctx.plan.run_id.clone(),
        step: step.name.clone(),
        worktree: worktree.to_path_buf(),
        trust: request_trust(ctx, step),
        inputs: collect_outputs(ctx, step),
        artifacts: collect_artifacts(ctx, step),
    }
}

/// Merged outputs of every `inputs_from` step, later steps winning.
fn collect_outputs(ctx: &RunCtx, step: &StepDef) -> BTreeMap<String, serde_json::Value> {
    let mut merged = BTreeMap::new();
    for name in &step.inputs_from {
        if let Some(result) = ctx.result_of(name) {
            merged.extend(result.outputs.clone());
        }
    }
    merged
}

/// Merged artifacts of every `inputs_from` step, by artifact name.
fn collect_artifacts(ctx: &RunCtx, step: &StepDef) -> BTreeMap<String, PathBuf> {
    let mut merged = BTreeMap::new();
    for name in &step.inputs_from {
        if let Some(result) = ctx.result_of(name) {
            merged.extend(
                result
                    .artifacts
                    .iter()
                    .map(|a| (a.name.clone(), a.path.clone())),
            );
        }
    }
    merged
}

/// A request's trust: the item's grade, lowered to the weakest input's
/// grade. The item seeds the fold because it is an input-less step's
/// only input; a step is only as trustworthy as its weakest input
/// (DESIGN.md section 9).
fn request_trust(ctx: &RunCtx, step: &StepDef) -> Trust {
    step.inputs_from
        .iter()
        .filter_map(|name| input_trust(ctx, name))
        .fold(ctx.plan.item.trust, Ord::min)
}

/// One input's grade, when data flowed from it. A step finished in
/// this run contributes its recorded grade. A step finished before a
/// resume has no result in memory — the log records outcomes, not
/// grades — so it conservatively counts as `Derived`, the weakest
/// grade a finished step can claim; the item-grade seed then keeps a
/// resumed request at or below the fresh run's grade. A step that
/// never ran contributes nothing: no data flowed from it.
fn input_trust(ctx: &RunCtx, step: &str) -> Option<Trust> {
    if let Some(result) = ctx.result_of(step) {
        return Some(result.trust);
    }
    ctx.outcome_of(step).map(|_| Trust::Derived)
}

/// The trust check: a step whose inputs grade below its minimum
/// escalates (fail closed), naming both grades.
pub(super) fn trust_check(plan: &RunPlan, step: &StepDef, request: &StepRequest) -> Option<String> {
    let minimum = min_trust(plan, step);
    if request.trust < minimum {
        return Some(format!(
            "step `{}` requires {minimum:?} trust but its inputs are {:?}",
            step.name, request.trust
        ));
    }
    None
}

/// The step's effective minimum trust: its own `trust`, else the
/// role's `min_trust` for agent steps. Deterministic steps are code and
/// default to `Untrusted`.
fn min_trust(plan: &RunPlan, step: &StepDef) -> Trust {
    if let Some(trust) = step.trust {
        return trust;
    }
    if step.kind == StepKind::Agent {
        return plan
            .roles
            .get(step.role.as_deref().unwrap_or_default())
            .map_or(Trust::Untrusted, |role| role.min_trust);
    }
    Trust::Untrusted
}

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
        StepKind::Decision => failed_step("decision steps do not run code"),
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
        log: Some(shared_log(LogSink::new(&step.name, &ctx.log))),
        cancel: Some(ctx.cancel_path()),
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

/// An agent step runs through the role's adapter; its outputs grade
/// `Derived` downstream no matter what the process claimed.
async fn agent(
    ctx: &RunCtx,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
) -> Execution {
    let Some(role) = role_for(ctx, step) else {
        return failed_step(&format!("step `{}` names an unknown role", step.name));
    };
    let sink = shared_log(LogSink::new(&step.name, &ctx.log));
    let mut execution =
        adapters::execute(role, step, request, timeout, ctx.secrets(), Some(sink)).await;
    execution.result.trust = Trust::Derived;
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

fn failure_result(message: &str) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Failure,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    }
}
