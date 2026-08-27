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

/// The escalate message for an edge-triggered escalation.
fn escalate_text(step: &StepDef, key: &str, detail: Option<&str>) -> String {
    let suffix = detail.map_or_else(String::new, |d| format!(": {d}"));
    format!("step `{}` ended `{key}` and escalated{suffix}", step.name)
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

#[cfg(test)]
mod tests {
    use super::{Route, route_after};
    use crate::config::{Pipeline, TERMINALS};
    use crate::contract::StepOutcome;

    /// A pipeline whose `start` step fails to `name`, and which *also* holds a
    /// step called `name`. `config::validate` refuses this shape; the engine is
    /// asked directly, because what is under test is which of the two `resolve`
    /// picks when both exist.
    ///
    /// The collision is the whole premise, so the test asserts it rather than
    /// trusting this string: without the second step the route below has only
    /// one candidate, and a green run would say nothing about which wins.
    fn failing_to(name: &str) -> Pipeline {
        let text = format!(
            "name: reserved\nsteps:\n  - name: start\n    type: deterministic\n    run: 'true'\n    next: done\n    on_failure: {name}\n  - name: {name}\n    type: deterministic\n    run: 'true'\n    next: done\n"
        );
        serde_yaml_ng::from_str(&text).expect("fixture pipeline")
    }

    /// Every name the loader reserves is a name `resolve` really settles first.
    ///
    /// `config::validate_pipeline::check_terminals` refuses a step named after
    /// a terminal, and its whole justification is the match above: `resolve`
    /// hard-codes the three arms and never reads `TERMINALS`, so nothing bound
    /// the reservation to the behaviour it was reserving against. Add a fourth
    /// route here and the loader silently stops reserving it — restoring the
    /// exact defect that rule was added to remove, from the other side.
    ///
    /// Each pair is `(the pipeline holds a step of that name, the route
    /// reached that step)`. The first half is asserted because it is the
    /// premise: a fixture that quietly stopped colliding would leave the
    /// second half true for the uninteresting reason.
    #[test]
    fn no_name_the_loader_reserves_can_route_to_a_step() {
        let observed: Vec<(bool, bool)> = TERMINALS
            .iter()
            .map(|name| {
                let pipeline = failing_to(name);
                let collides = pipeline.steps.iter().any(|step| step.name == *name);
                let start = &pipeline.steps[0];
                let route = route_after(start, StepOutcome::Failure, None, &pipeline);
                (collides, matches!(route, Route::Step(_)))
            })
            .collect();

        assert_eq!(observed, vec![(true, false); TERMINALS.len()]);
    }
}
