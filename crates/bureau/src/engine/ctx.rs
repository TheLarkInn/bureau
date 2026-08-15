//! The shared run context: the machine's mutable state (`RunCtx`) and
//! the worktree phase's products (`WtCtx`), factored out so `machine`
//! and `execute` both depend here instead of on each other.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{RunPlan, edge, resume, stream};
use crate::contract::{StepOutcome, StepResult};
use crate::git::Worktree;

/// The most one step may claim toward a run's cost total.
const MAX_STEP_COST_USD: f64 = 25.0;

/// Clamps a step's claimed cost into `0.0..=MAX_STEP_COST_USD`.
///
/// The claim is advisory: the agent authors it, so a malfunctioning or
/// prompt-injected agent could report 0.0 and dodge the
/// `max_cost_per_day_usd` budget. The structural spend guards are
/// `max_runs_per_hour`/`max_runs_per_day`; adapter-measured cost is
/// future work. NaN (on which `f64::clamp` panics) clamps to 0.0 — the
/// contract layer cannot deserialize NaN from JSON, but the clamp does
/// not trust that.
const fn clamp_step_cost(claimed: f64) -> f64 {
    if claimed.is_nan() {
        0.0
    } else {
        claimed.clamp(0.0, MAX_STEP_COST_USD)
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

/// Mutable run state threaded through the machine.
#[derive(bon::Builder)]
pub(super) struct RunCtx {
    /// Everything one run needs.
    pub(super) plan: RunPlan,
    /// The run log, shared with live step sinks.
    pub(super) log: stream::Shared,
    /// Wall clock (millis since the Unix epoch) for git and step spawns.
    pub(super) clock: fn() -> u64,
    /// Summed step cost.
    pub(super) cost_usd: f64,
    /// Entries per step name, from history plus this run.
    pub(super) attempts: BTreeMap<String, u32>,
    /// Latest outcome per step (decision `over` reads this).
    pub(super) outcomes: BTreeMap<String, StepOutcome>,
    /// Latest result per step (`inputs_from` reads this).
    pub(super) results: BTreeMap<String, StepResult>,
    /// Where the machine starts or resumes.
    pub(super) start: edge::Route,
}

impl RunCtx {
    /// Assembles the machine's state from the plan and the replay.
    pub(super) fn new(
        plan: &RunPlan,
        log: super::log::Appender,
        history: resume::History,
        clock: fn() -> u64,
    ) -> Self {
        Self {
            plan: plan.clone(),
            log: Arc::new(Mutex::new(log)),
            clock,
            cost_usd: 0.0,
            attempts: history.attempts,
            outcomes: history.outcomes,
            results: BTreeMap::new(),
            start: history.start,
        }
    }

    /// Records a finished step's result for routing and data flow.
    pub(super) fn record(&mut self, step: &str, result: StepResult) {
        self.cost_usd += clamp_step_cost(result.cost_usd);
        self.outcomes.insert(step.to_owned(), result.outcome);
        self.results.insert(step.to_owned(), result);
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

#[cfg(test)]
mod tests {
    use super::{MAX_STEP_COST_USD, clamp_step_cost};

    /// The clamp's truth table: NaN and negative claims zero out,
    /// in-range claims pass through, anything past the cap clamps to it.
    /// Compared by bits: the clamp only pins to exact bounds or passes
    /// its input through unchanged, so equality is exact (and clippy's
    /// `float_cmp` stays quiet).
    #[test]
    fn claimed_cost_clamps_into_bounds() {
        let cases = [
            (f64::NAN, 0.0),
            (f64::NEG_INFINITY, 0.0),
            (-1.0, 0.0),
            (0.0, 0.0),
            (0.42, 0.42),
            (MAX_STEP_COST_USD, MAX_STEP_COST_USD),
            (1000.0, MAX_STEP_COST_USD),
            (f64::INFINITY, MAX_STEP_COST_USD),
        ];
        for (claimed, want) in cases {
            let got = clamp_step_cost(claimed);
            assert_eq!(got.to_bits(), want.to_bits(), "claimed {claimed}");
        }
    }
}
