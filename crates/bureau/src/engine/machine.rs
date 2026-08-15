//! The state machine: step entry, the gates, and the CANCEL check.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{RunPlan, edge, execute, resume, stream};
use crate::adapters::{Execution, Usage};
use crate::config::{Repo, StepDef, StepKind};
use crate::contract::{StepOutcome, StepRequest, StepResult};
use crate::git::Worktree;
use crate::runlog::{self, EventKind};

/// Mutable run state threaded through the machine.
pub(super) struct RunCtx {
    /// Everything one run needs.
    pub(super) plan: RunPlan,
    /// The run log, shared with live step sinks.
    pub(super) log: stream::Shared,
    /// Summed step cost.
    pub(super) cost_usd: f64,
    /// Entries per step name, from history plus this run.
    attempts: BTreeMap<String, u32>,
    /// Latest outcome per step (decision `over` reads this).
    outcomes: BTreeMap<String, StepOutcome>,
    /// Latest result per step (`inputs_from` reads this).
    results: BTreeMap<String, StepResult>,
    /// Adapter-measured usage per step.
    usages: BTreeMap<String, Usage>,
    /// Where the machine starts or resumes.
    start: edge::Route,
}

impl RunCtx {
    /// Assembles the machine's state from the plan and the replay.
    pub(super) fn new(plan: &RunPlan, log: super::log::Appender, history: resume::History) -> Self {
        Self {
            plan: plan.clone(),
            log: Arc::new(Mutex::new(log)),
            cost_usd: 0.0,
            attempts: history.attempts,
            outcomes: history.outcomes,
            results: BTreeMap::new(),
            usages: BTreeMap::new(),
            start: history.start,
        }
    }

    /// Records a finished step's result for routing and data flow.
    fn record(&mut self, step: &str, execution: Execution) {
        let Execution { result, usage } = execution;
        self.cost_usd += measured_cost(&usage);
        self.outcomes.insert(step.to_owned(), result.outcome);
        self.results.insert(step.to_owned(), result);
        self.usages.insert(step.to_owned(), usage);
    }

    /// The latest outcome of `step`, when it has finished one.
    pub(super) fn outcome_of(&self, step: &str) -> Option<StepOutcome> {
        self.outcomes.get(step).copied()
    }

    /// The latest result of `step`, when this run has it.
    pub(super) fn result_of(&self, step: &str) -> Option<&StepResult> {
        self.results.get(step)
    }

    /// The scrub list: every resolved credential value.
    pub(super) fn secrets(&self) -> Vec<crate::process::Secret> {
        self.plan.credentials.values().cloned().collect()
    }
}

fn measured_cost(usage: &Usage) -> f64 {
    usage
        .cost_usd
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
        .unwrap_or(0.0)
}

/// The worktree phase's products.
pub(super) struct WtCtx {
    /// The run's worktree guard; dropping it tears the worktree down.
    pub(super) worktree: Worktree,
    /// The bare mirror the worktree was cut from.
    pub(super) mirror: PathBuf,
    /// The run branch.
    pub(super) branch: String,
    /// `HEAD` at worktree creation; finalize compares against it.
    pub(super) start_head: String,
}

/// Why the machine stopped.
pub(super) enum Stop {
    /// Reached the `done` terminal.
    Done,
    /// `abort`, a fail-closed route, or the CANCEL marker.
    Fail(String),
    /// `escalate`: comment for a human, outcome Blocked.
    Escalate(String),
}

/// One loop iteration's verdict.
enum Turn {
    /// Keep routing.
    Next(edge::Route),
    /// The run stops.
    Stop(Stop),
}

/// The machine loop: route, check CANCEL, run steps, stop at terminals.
pub(super) async fn run_loop(ctx: &mut RunCtx, wt: &WtCtx) -> Stop {
    let mut route = ctx.start.clone();
    loop {
        match advance(ctx, wt, route).await {
            Turn::Next(next) => route = next,
            Turn::Stop(stop) => return stop,
        }
    }
}

/// One iteration: the between-steps CANCEL check, then the route.
async fn advance(ctx: &mut RunCtx, wt: &WtCtx, route: edge::Route) -> Turn {
    if cancelled(ctx) {
        return Turn::Stop(Stop::Fail("cancelled".to_owned()));
    }
    match route {
        edge::Route::Step(name) => step_turn(ctx, wt, &name).await,
        edge::Route::Done => Turn::Stop(Stop::Done),
        edge::Route::Fail(message) => Turn::Stop(Stop::Fail(message)),
        edge::Route::Escalate(message) => Turn::Stop(Stop::Escalate(message)),
    }
}

/// The CANCEL marker `bureau cancel <run-id>` writes into the run dir.
fn cancelled(ctx: &RunCtx) -> bool {
    stream::lock(&ctx.log).dir().join("CANCEL").exists()
}

/// Enters one step: decisions route for free, code steps run.
async fn step_turn(ctx: &mut RunCtx, wt: &WtCtx, name: &str) -> Turn {
    let Some(step) = ctx
        .plan
        .pipeline
        .steps
        .iter()
        .find(|s| s.name == name)
        .cloned()
    else {
        return Turn::Stop(Stop::Fail(format!("unknown step `{name}`")));
    };
    if step.kind == StepKind::Decision {
        return Turn::Next(decision_route(ctx, &step));
    }
    code_route(ctx, wt, &step).await
}

/// Runs one code step: attempts check, trust check, execute, record.
async fn code_route(ctx: &mut RunCtx, wt: &WtCtx, step: &StepDef) -> Turn {
    if let Some(reason) = attempts_check(ctx, step) {
        return Turn::Stop(Stop::Escalate(reason));
    }
    let request = execute::build_request(ctx, step, wt.worktree.path());
    if let Some(reason) = execute::trust_check(&ctx.plan, step, &request) {
        return Turn::Stop(Stop::Escalate(reason));
    }
    let execution = run_step(ctx, wt, step, &request).await;
    let (outcome, detail) = (execution.result.outcome, execution.result.message.clone());
    ctx.record(&step.name, execution);
    Turn::Next(edge::route_after(
        step,
        outcome,
        Some(&detail),
        &ctx.plan.pipeline,
    ))
}

/// The attempts check: a step entered `max_attempts` times escalates.
fn attempts_check(ctx: &RunCtx, step: &StepDef) -> Option<String> {
    let entries = ctx.attempts.get(&step.name).copied().unwrap_or(0);
    if entries >= step.max_attempts {
        return Some(format!("step `{}` exceeded max attempts", step.name));
    }
    None
}

/// Executes a step between its started and finished events.
async fn run_step(
    ctx: &mut RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    request: &StepRequest,
) -> Execution {
    append(
        ctx,
        EventKind::StepStarted,
        runlog::step_started(&step.name),
    );
    *ctx.attempts.entry(step.name.clone()).or_insert(0) += 1;
    let result = execute::execute(ctx, wt, step, request).await;
    let finished = runlog::step_finished(&step.name, result.result.outcome);
    append(ctx, EventKind::StepFinished, finished);
    result
}

/// Routes a decision step on the outcome of the step it watches.
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

/// Appends an event, ignoring failures: a run never crashes over its
/// own bookkeeping.
pub(super) fn append(ctx: &RunCtx, kind: EventKind, data: serde_json::Value) {
    let _ = stream::lock(&ctx.log).append(kind, data);
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
mod tests {
    use super::measured_cost;
    use crate::adapters::Usage;

    #[test]
    fn only_valid_adapter_cost_is_counted() {
        let cases = [
            (None, 0.0_f64),
            (Some(f64::NAN), 0.0),
            (Some(-1.0), 0.0),
            (Some(0.42), 0.42),
            (Some(1_000.0), 1_000.0),
        ];
        for (cost_usd, want) in cases {
            let usage = Usage {
                cost_usd,
                ..Usage::default()
            };
            let got = measured_cost(&usage);
            assert_eq!(got.to_bits(), want.to_bits(), "measured {cost_usd:?}");
        }
    }
}
