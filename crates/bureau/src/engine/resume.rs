//! Resume: replaying the event log into where the machine continues.
//! The log records outcomes, not outputs or trust grades, so a resumed
//! run re-derives routing from outcomes, earlier steps' outputs are
//! empty, and their grades conservatively count as `Derived` when a
//! later step's request trust is folded (see `execute::request_trust`).

use std::collections::BTreeMap;

use super::edge::{self, Route};
use crate::adapters::Usage;
use crate::config::Pipeline;
use crate::contract::{StepOutcome, StepResult};
use crate::forge::Pr;
use crate::runlog::GroupRecord;
use crate::runlog::{self, Event, RunFinishedData, RunState, RunStatus};

/// What a replayed log says about how to proceed.
pub(super) enum Replay {
    /// The log holds `run_finished`; return the outcome untouched.
    Finished(RunFinishedData),
    /// Continue from the replayed history.
    Resume(History),
}

/// Replayed step history.
pub(super) struct History {
    /// Entries per step name, from `step_started` records.
    pub(super) attempts: BTreeMap<String, u32>,
    /// Latest finished outcome per step.
    pub(super) outcomes: BTreeMap<String, StepOutcome>,
    /// Full results from completed steps.
    pub(super) results: BTreeMap<String, StepResult>,
    /// Adapter usage from completed steps.
    pub(super) usages: BTreeMap<String, Usage>,
    /// Partial or finished concurrent groups.
    pub(super) groups: BTreeMap<String, GroupRecord>,
    /// Latest durable branch checkpoint.
    pub(super) checkpoint: Option<String>,
    /// Run branch base before step changes.
    pub(super) base_commit: Option<String>,
    /// Exact pushed commit, if finalization reached it.
    pub(super) pushed_commit: Option<String>,
    /// Created/adopted PR.
    pub(super) pr: Option<Pr>,
    /// Original wall-clock start.
    pub(super) started_at_ms: u64,
    /// Where the machine continues.
    pub(super) start: Route,
    /// Whether the log already holds `run_started`.
    pub(super) started: bool,
}

impl History {
    /// Empty history starting at `start`.
    pub(super) const fn fresh(start: Route, started: bool) -> Self {
        Self {
            attempts: BTreeMap::new(),
            outcomes: BTreeMap::new(),
            results: BTreeMap::new(),
            usages: BTreeMap::new(),
            groups: BTreeMap::new(),
            checkpoint: None,
            base_commit: None,
            pushed_commit: None,
            pr: None,
            started_at_ms: 0,
            start,
            started,
        }
    }
}

/// The pipeline's entry step.
pub(super) fn entry(pipeline: &Pipeline) -> Route {
    pipeline.steps.first().map_or_else(
        || Route::Fail("pipeline has no steps".to_owned()),
        |step| Route::Step(step.name.clone()),
    )
}

/// Replays events into a resume decision.
pub(super) fn replay(events: Vec<Event>, pipeline: &Pipeline) -> Replay {
    let Some(state) = runlog::replay(events) else {
        return Replay::Resume(History::fresh(entry(pipeline), false));
    };
    if let RunStatus::Finished(_) = state.status {
        return Replay::Finished(
            state
                .finished
                .clone()
                .unwrap_or_else(|| legacy_finished(&state)),
        );
    }
    Replay::Resume(history_from(&state, pipeline))
}

/// Folds step history into attempt counts, outcomes, and the
/// continuation route.
fn history_from(state: &RunState, pipeline: &Pipeline) -> History {
    let steps = step_history(state);
    History {
        attempts: steps.attempts,
        outcomes: steps.outcomes,
        results: steps.results,
        usages: steps.usages,
        groups: state.groups.clone(),
        checkpoint: state.checkpoint.clone(),
        base_commit: state.base_commit.clone(),
        pushed_commit: state.pushed_commit.clone(),
        pr: state.pr.clone(),
        started_at_ms: state.started_at_ms,
        start: next_route(state, pipeline),
        started: true,
    }
}

struct StepHistory {
    attempts: BTreeMap<String, u32>,
    outcomes: BTreeMap<String, StepOutcome>,
    results: BTreeMap<String, StepResult>,
    usages: BTreeMap<String, Usage>,
}

fn step_history(state: &RunState) -> StepHistory {
    let mut history = StepHistory {
        attempts: BTreeMap::new(),
        outcomes: BTreeMap::new(),
        results: BTreeMap::new(),
        usages: BTreeMap::new(),
    };
    for record in &state.steps {
        add_record(&mut history, record);
    }
    history
}

fn add_record(history: &mut StepHistory, record: &crate::runlog::StepRecord) {
    *history.attempts.entry(record.step.clone()).or_insert(0) += 1;
    if let Some(outcome) = record.outcome {
        history.outcomes.insert(record.step.clone(), outcome);
    }
    if let Some(result) = record.result.clone() {
        history.results.insert(record.step.clone(), result);
    }
    if let Some(usage) = record.usage.clone() {
        history.usages.insert(record.step.clone(), usage);
    }
}

fn legacy_finished(state: &RunState) -> RunFinishedData {
    let outcome = match state.status {
        RunStatus::Finished(outcome) => outcome,
        RunStatus::Running => StepOutcome::Failure,
    };
    RunFinishedData {
        outcome,
        message: format!("run already finished: {}", edge::outcome_key(outcome)),
        cost_usd: 0.0,
        pr: state.pr.clone(),
        disposition: None,
    }
}

/// Where a partially-recorded run continues: re-enter an interrupted
/// step (its start already consumed an attempt), else route the last
/// finished step's outcome.
fn next_route(state: &RunState, pipeline: &Pipeline) -> Route {
    let Some(last) = state.steps.last() else {
        return entry(pipeline);
    };
    last.outcome.map_or_else(
        || Route::Step(last.step.clone()),
        |outcome| edge::route_named(pipeline, &last.step, outcome),
    )
}
