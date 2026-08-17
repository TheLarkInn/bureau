//! Step request data flow and trust gates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::RunPlan;
use super::context::RunCtx;
use crate::config::{StepDef, StepKind};
use crate::contract::{SCHEMA_VERSION, StepRequest, Trust};

fn input_kind(ctx: &RunCtx, name: &str) -> Option<StepKind> {
    ctx.plan
        .pipeline
        .steps
        .iter()
        .find(|step| step.name == name)
        .map(|step| step.kind)
}

fn input_trust(ctx: &RunCtx, step: &str) -> Option<Trust> {
    if let Some(result) = ctx.result_of(step) {
        return Some(result.trust);
    }
    ctx.outcome_of(step).map(|_| Trust::Derived)
}

fn collect_outputs(ctx: &RunCtx, step: &StepDef) -> BTreeMap<String, serde_json::Value> {
    let mut merged = BTreeMap::new();
    for name in &step.inputs_from {
        if let Some(result) = ctx.result_of(name) {
            if input_kind(ctx, name) == Some(StepKind::Concurrent) {
                let value = serde_json::to_value(&result.outputs).unwrap_or_default();
                merged.insert(name.clone(), value);
            } else {
                merged.extend(result.outputs.clone());
            }
        }
    }
    merged
}

fn collect_artifacts(ctx: &RunCtx, step: &StepDef) -> BTreeMap<String, PathBuf> {
    let mut merged = BTreeMap::new();
    for name in &step.inputs_from {
        if let Some(result) = ctx.result_of(name) {
            let concurrent = input_kind(ctx, name) == Some(StepKind::Concurrent);
            merged.extend(result.artifacts.iter().map(|artifact| {
                let key = if concurrent {
                    format!("{name}.{}", artifact.name)
                } else {
                    artifact.name.clone()
                };
                (key, artifact.path.clone())
            }));
        }
    }
    merged
}

fn request_trust(ctx: &RunCtx, step: &StepDef) -> Trust {
    step.inputs_from
        .iter()
        .filter_map(|name| input_trust(ctx, name))
        .fold(ctx.plan.item.trust, Ord::min)
}

pub(super) fn build(ctx: &RunCtx, step: &StepDef, worktree: &Path) -> StepRequest {
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
