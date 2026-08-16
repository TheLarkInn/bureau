//! The state machine: step entry, the gates, and the CANCEL check.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{RunPlan, checkpoint, control, deadline, edge, execute, resume, stream};
use crate::adapters::{Execution, Usage};
use crate::config::{Repo, StepDef};
use crate::contract::{StepOutcome, StepRequest, StepResult};
use crate::git::Worktree;
use crate::runlog::GroupRecord;

mod route;
use crate::runlog::{self, EventKind};

/// Mutable run state threaded through the machine.
#[derive(Clone)]
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
    /// Partial or finished concurrent group state.
    pub(super) groups: BTreeMap<String, GroupRecord>,
    /// Latest durable branch checkpoint.
    pub(super) checkpoint: Option<String>,
    /// Run branch base before step changes.
    pub(super) base_commit: Option<String>,
    /// Exact pushed commit.
    pub(super) pushed_commit: Option<String>,
    /// Created/adopted PR.
    pub(super) pr: Option<crate::forge::Pr>,
    /// Complete-run deadline.
    deadline: tokio::time::Instant,
    /// Where the machine starts or resumes.
    start: edge::Route,
    /// Whether the immutable run snapshot is already durable.
    pub(super) started: bool,
}

impl RunCtx {
    /// Assembles the machine's state from the plan and the replay.
    pub(super) fn new(
        plan: &RunPlan,
        log: crate::runlog::RunLog,
        history: resume::History,
    ) -> Self {
        let deadline = deadline::at(history.started_at_ms, plan.assignment.limits.max_run_hours);
        Self {
            plan: plan.clone(),
            log: Arc::new(Mutex::new(log)),
            cost_usd: history.usages.values().map(measured_cost).sum(),
            attempts: history.attempts,
            outcomes: history.outcomes,
            results: history.results,
            usages: history.usages,
            groups: history.groups,
            checkpoint: history.checkpoint,
            base_commit: history.base_commit,
            pushed_commit: history.pushed_commit,
            pr: history.pr,
            deadline,
            start: history.start,
            started: history.started,
        }
    }

    /// Records a finished step's result for routing and data flow.
    pub(super) fn record(&mut self, step: &str, execution: Execution) {
        let Execution { result, usage, .. } = execution;
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

    pub(super) fn remaining(&self) -> std::time::Duration {
        deadline::remaining(self.deadline)
    }

    pub(super) fn cancel_path(&self) -> PathBuf {
        stream::lock(&self.log).dir().join("CANCEL")
    }

    pub(super) fn begin_attempt(&mut self, step: &str) {
        *self.attempts.entry(step.to_owned()).or_insert(0) += 1;
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
    append(
        ctx,
        EventKind::StepStarted,
        runlog::step_started(&step.name),
    );
    *ctx.attempts.entry(step.name.clone()).or_insert(0) += 1;
    let mut result = execute::execute(ctx, wt, step, request).await;
    if result.is_halted() || control::ownership_reason(ctx).is_some() {
        return result.halt();
    }
    checkpoint::save_result(ctx, wt, step, &mut result).await;
    let finished = runlog::step_finished_full(&step.name, &result);
    append(ctx, EventKind::StepFinished, finished);
    result
}

/// Appends an event, ignoring failures: a run never crashes over its
/// own bookkeeping.
pub(super) fn append(ctx: &RunCtx, kind: EventKind, data: serde_json::Value) {
    if control::ownership_reason(ctx).is_some() {
        return;
    }
    let _ = stream::lock(&ctx.log).append(kind, data);
}

pub(super) fn run_dir(ctx: &RunCtx) -> PathBuf {
    stream::lock(&ctx.log).dir().to_path_buf()
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
