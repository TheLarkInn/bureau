//! Deterministic aggregate result for one concurrent group.

use std::collections::BTreeMap;

use crate::adapters::{Execution, Usage};
use crate::contract::{Artifact, SCHEMA_VERSION, StepOutcome, StepResult, Trust};

pub(super) fn aggregate(
    results: &BTreeMap<String, Execution>,
    cancelled: &BTreeMap<String, String>,
) -> Execution {
    let halted = results.values().find(|execution| execution.is_halted());
    let outcome = aggregate_outcome(results, cancelled, halted);
    let trust = aggregate_trust(results);
    let result = StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: outputs(results, cancelled),
        artifacts: artifacts(results),
        trust,
        message: message(results, cancelled, outcome),
    };
    let execution = Execution::new(result, usage(results));
    if halted.is_some() {
        execution.halt()
    } else {
        execution
    }
}

fn aggregate_outcome(
    results: &BTreeMap<String, Execution>,
    cancelled: &BTreeMap<String, String>,
    halted: Option<&Execution>,
) -> StepOutcome {
    halted.map_or_else(
        || {
            outcome(
                results.values().map(|value| value.result.outcome),
                !cancelled.is_empty(),
            )
        },
        |execution| execution.result.outcome,
    )
}

fn aggregate_trust(results: &BTreeMap<String, Execution>) -> Trust {
    results
        .values()
        .map(|execution| execution.result.trust)
        .min()
        .unwrap_or(Trust::Derived)
}

fn outcome(outcomes: impl Iterator<Item = StepOutcome>, cancelled: bool) -> StepOutcome {
    let outcomes: Vec<_> = outcomes.collect();
    if cancelled || outcomes.contains(&StepOutcome::Failure) {
        StepOutcome::Failure
    } else if outcomes.contains(&StepOutcome::Blocked) {
        StepOutcome::Blocked
    } else if outcomes
        .iter()
        .all(|outcome| *outcome == StepOutcome::NoWork)
    {
        StepOutcome::NoWork
    } else {
        StepOutcome::Success
    }
}

fn outputs(
    results: &BTreeMap<String, Execution>,
    cancelled: &BTreeMap<String, String>,
) -> BTreeMap<String, serde_json::Value> {
    let mut outputs: BTreeMap<_, _> = results
        .iter()
        .map(|(name, execution)| (name.clone(), member_value(execution)))
        .collect();
    outputs.extend(cancelled.iter().map(|(name, reason)| {
        (
            name.clone(),
            serde_json::json!({"cancelled": true, "reason": reason}),
        )
    }));
    outputs
}

fn member_value(execution: &Execution) -> serde_json::Value {
    let result = &execution.result;
    serde_json::json!({
        "outcome": outcome_name(result.outcome),
        "outputs": result.outputs,
        "message": result.message,
        "trust": result.trust,
        "artifacts": result.artifacts,
    })
}

fn artifacts(results: &BTreeMap<String, Execution>) -> Vec<Artifact> {
    results
        .iter()
        .flat_map(|(member, execution)| {
            execution
                .result
                .artifacts
                .iter()
                .map(move |artifact| Artifact {
                    name: format!("{member}.{}", artifact.name),
                    path: artifact.path.clone(),
                })
        })
        .collect()
}

fn usage(results: &BTreeMap<String, Execution>) -> Usage {
    let values: Vec<_> = results.values().map(|execution| &execution.usage).collect();
    Usage {
        provider: "concurrent".to_owned(),
        input_tokens: sum_u64(&values, |usage| usage.input_tokens),
        output_tokens: sum_u64(&values, |usage| usage.output_tokens),
        credits: sum_f64(&values, |usage| usage.credits),
        cost_usd: sum_f64(&values, |usage| usage.cost_usd),
        cost_basis: Some("sum_of_member_adapter_usage".to_owned()),
    }
}

fn sum_u64(values: &[&Usage], field: impl Fn(&Usage) -> Option<u64>) -> Option<u64> {
    values.iter().try_fold(0_u64, |sum, usage| {
        field(usage).map(|value| sum.saturating_add(value))
    })
}

fn sum_f64(values: &[&Usage], field: impl Fn(&Usage) -> Option<f64>) -> Option<f64> {
    values
        .iter()
        .try_fold(0.0, |sum, usage| field(usage).map(|value| sum + value))
}

fn message(
    results: &BTreeMap<String, Execution>,
    cancelled: &BTreeMap<String, String>,
    outcome: StepOutcome,
) -> String {
    format!(
        "{} concurrent members finished, {} cancelled, aggregate `{}`",
        results.len(),
        cancelled.len(),
        outcome_name(outcome)
    )
}

const fn outcome_name(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Success => "success",
        StepOutcome::Failure => "failure",
        StepOutcome::Blocked => "blocked",
        StepOutcome::NoWork => "no-work",
    }
}
