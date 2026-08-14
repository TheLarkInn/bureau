//! Running one step: request assembly, the trust gate, and the two
//! code-running kinds (DESIGN.md layer 4 and section 9).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::RunPlan;
use super::machine::{RunCtx, WtCtx};
use super::stream::LogSink;
use crate::adapters;
use crate::config::{StepDef, StepKind};
use crate::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust};
use crate::process::{Secret, SpawnRequest, SpawnResult, shared_log, spawn};

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

/// A request's trust: the item's grade raised by any graded input.
fn request_trust(ctx: &RunCtx, step: &StepDef) -> Trust {
    step.inputs_from
        .iter()
        .filter_map(|name| ctx.result_of(name))
        .map(|result| result.trust)
        .fold(ctx.plan.item.trust, Ord::max)
}

/// The trust gate: a step whose inputs grade below its minimum
/// escalates (fail closed), naming both grades.
pub(super) fn trust_gate(plan: &RunPlan, step: &StepDef, request: &StepRequest) -> Option<String> {
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
) -> StepResult {
    match step.kind {
        StepKind::Deterministic => deterministic(ctx, wt, step, request).await,
        StepKind::Agent => agent(ctx, step, request).await,
        StepKind::Decision => failed_step("decision steps do not run code"),
    }
}

/// A deterministic step: `sh -c <run>` in the worktree under the
/// layer-0 contract. Resolved credentials arrive as
/// `BUREAU_CREDENTIAL_<NAME>` and join the scrub list.
async fn deterministic(
    ctx: &RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
) -> StepResult {
    let spawned = spawn(SpawnRequest {
        argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            step.run.clone().unwrap_or_default(),
        ],
        dir: wt.worktree.path().to_path_buf(),
        env: credential_env(&ctx.plan.credentials),
        stdin: request.to_json().unwrap_or_default(),
        timeout: Duration::from_secs(step.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
        secrets: ctx.secrets(),
        log: Some(shared_log(LogSink::new(&step.name, &ctx.log))),
    })
    .await;
    derive_result(request.trust, &spawned)
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
async fn agent(ctx: &RunCtx, step: &StepDef, request: &StepRequest) -> StepResult {
    let Some(role) = step
        .role
        .as_deref()
        .and_then(|name| ctx.plan.roles.get(name))
    else {
        return failed_step(&format!("step `{}` names an unknown role", step.name));
    };
    let sink = shared_log(LogSink::new(&step.name, &ctx.log));
    let mut result = adapters::execute(role, step, request, ctx.secrets(), Some(sink)).await;
    result.trust = Trust::Derived;
    result
}

/// A synthetic failure for a step that cannot run at all.
fn failed_step(message: &str) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Failure,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        cost_usd: 0.0,
        message: message.to_owned(),
    }
}

/// The complete deterministic-step env: every resolved credential as
/// `BUREAU_CREDENTIAL_<NAME>`.
fn credential_env(credentials: &BTreeMap<String, Secret>) -> BTreeMap<String, String> {
    credentials
        .iter()
        .map(|(name, secret)| (env_name(name), secret.expose().to_owned()))
        .collect()
}

/// `api-token` becomes `BUREAU_CREDENTIAL_API_TOKEN` (the layer-0
/// convention).
fn env_name(reference: &str) -> String {
    format!(
        "BUREAU_CREDENTIAL_{}",
        reference.to_uppercase().replace('-', "_")
    )
}
