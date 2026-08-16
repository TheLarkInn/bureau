//! Edge resolution: an outcome maps to a target, a target to a route.
//! Anything missing or unknown fails closed (DESIGN.md layer 4).

use crate::config::{Pipeline, StepDef};
use crate::contract::StepOutcome;

/// A routing target: the next step, or a terminal with its message.
#[derive(Debug, Clone)]
pub(super) enum Route {
    /// Run the named step next.
    Step(String),
    /// The `done` terminal: finalize (push + PR).
    Done,
    /// The `abort` terminal (or a fail-closed routing problem).
    Fail(String),
    /// The `escalate` terminal: comment on the item, then Blocked.
    Escalate(String),
}

/// The `on` map key for an outcome (kebab-case, per the schema).
pub(super) const fn outcome_key(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Success => "success",
        StepOutcome::Failure => "failure",
        StepOutcome::Blocked => "blocked",
        StepOutcome::NoWork => "no-work",
    }
}

/// Routes a finished code step along its outcome's edge; a missing edge
/// fails closed to `abort`.
pub(super) fn route_after(
    step: &StepDef,
    outcome: StepOutcome,
    detail: Option<&str>,
    pipeline: &Pipeline,
) -> Route {
    let edge = match outcome {
        StepOutcome::Success => step.next.as_deref(),
        StepOutcome::Failure => step.on_failure.as_deref(),
        StepOutcome::Blocked => step.on_blocked.as_deref(),
        StepOutcome::NoWork => step.on_no_work.as_deref(),
    };
    resolve(step, edge, outcome, detail, pipeline)
}

/// Routes a decision step on the outcome of the step it watches.
pub(super) fn route_decision(step: &StepDef, outcome: StepOutcome, pipeline: &Pipeline) -> Route {
    let edge = step.on.get(outcome_key(outcome)).map(String::as_str);
    resolve(step, edge, outcome, None, pipeline)
}

/// Routes a resumed run's last finished step, looked up by name.
pub(super) fn route_named(pipeline: &Pipeline, name: &str, outcome: StepOutcome) -> Route {
    let Some(step) = pipeline.steps.iter().find(|s| s.name == name) else {
        return Route::Fail(format!("unknown step `{name}`"));
    };
    route_after(step, outcome, None, pipeline)
}

/// Resolves an edge target to a route; anything unknown fails closed.
fn resolve(
    step: &StepDef,
    edge: Option<&str>,
    outcome: StepOutcome,
    detail: Option<&str>,
    pipeline: &Pipeline,
) -> Route {
    let key = outcome_key(outcome);
    let Some(target) = edge else {
        return Route::Fail(format!(
            "step `{}` has no edge for outcome `{key}`; aborting",
            step.name
        ));
    };
    match target {
        "done" => Route::Done,
        "abort" => Route::Fail(format!("step `{}` routed to abort on `{key}`", step.name)),
        "escalate" => Route::Escalate(escalate_text(step, key, detail)),
        name if pipeline.steps.iter().any(|s| s.name == name) => Route::Step(name.to_owned()),
        other => Route::Fail(format!(
            "step `{}` routes to unknown target `{other}`; aborting",
            step.name
        )),
    }
}

/// The escalate message for an edge-triggered escalation.
fn escalate_text(step: &StepDef, key: &str, detail: Option<&str>) -> String {
    let suffix = detail.map_or_else(String::new, |d| format!(": {d}"));
    format!("step `{}` ended `{key}` and escalated{suffix}", step.name)
}
