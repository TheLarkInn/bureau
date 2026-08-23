//! Mutable run state shared by the machine and its helpers: the run
//! context, the worktree phase's products, the lease-ownership check,
//! and event appends. The low-level module under `engine` so the machine's siblings
//! depend on this module instead of on the machine itself.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{RunPlan, deadline, edge, resume, stream};
use crate::adapters::{Execution, Usage};
use crate::contract::{StepOutcome, StepResult};
use crate::git::Worktree;
use crate::runlog::{EventKind, GroupRecord};

/// The counted cost of one step's adapter usage; unmeasured or invalid
/// values count as zero.
pub(super) fn measured_cost(usage: &Usage) -> f64 {
    usage
        .cost_usd
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
        .unwrap_or(0.0)
}

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
    pub(super) attempts: BTreeMap<String, u32>,
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
    pub(super) start: edge::Route,
    /// Whether the immutable run snapshot is already durable.
    pub(super) started: bool,
    /// Forge identity per credential, verified once before the first
    /// spawn and pinned into `run_started`.
    pub(super) verified: BTreeMap<String, String>,
}

impl RunCtx {
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

    /// The run-directory marker that pauses the run at a step boundary.
    /// The name stays a literal here so this module and `control` do
    /// not depend on each other.
    pub(super) fn pause_path(&self) -> PathBuf {
        stream::lock(&self.log).dir().join("PAUSE")
    }

    pub(super) fn begin_attempt(&mut self, step: &str) {
        *self.attempts.entry(step.to_owned()).or_insert(0) += 1;
    }
}

/// Assembles the machine's state from the plan and the replay.
pub(super) fn run_ctx(
    plan: &RunPlan,
    log: crate::runlog::RunLog,
    history: resume::History,
) -> RunCtx {
    let deadline = deadline::at(history.started_at_ms, plan.assignment.limits.max_run_hours);
    RunCtx {
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
        verified: BTreeMap::new(),
    }
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

/// The run's lease-ownership check: `Some(reason)` when this process no
/// longer owns (or cannot confirm it owns) the run.
pub(super) fn ownership_reason(ctx: &RunCtx) -> Option<String> {
    let owner = ctx.plan.lease.as_ref()?;
    match owner.owns() {
        Ok(true) => None,
        Ok(false) => Some("run lease ownership was lost".to_owned()),
        Err(error) => Some(format!(
            "run lease ownership could not be confirmed: {error}"
        )),
    }
}

/// Appends an event, ignoring failures: a run never crashes over its
/// own bookkeeping.
pub(super) fn append(ctx: &RunCtx, kind: EventKind, data: serde_json::Value) {
    if ownership_reason(ctx).is_some() {
        return;
    }
    let _ = stream::lock(&ctx.log).append(kind, data);
}

/// The run's durable directory.
pub(super) fn run_dir(ctx: &RunCtx) -> PathBuf {
    stream::lock(&ctx.log).dir().to_path_buf()
}
