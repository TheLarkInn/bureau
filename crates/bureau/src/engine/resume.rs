//! Resume: replaying the event log into where the machine continues.
//! The log records outcomes, not outputs or trust grades, so a resumed
//! run re-derives routing from outcomes, earlier steps' outputs are
//! empty, and their grades conservatively count as `Derived` when a
//! later step's request trust is folded (see `execute::request_trust`).

use std::collections::BTreeMap;

use super::edge::{self, Route};
use crate::config::Pipeline;
use crate::contract::StepOutcome;
use crate::runlog::{self, Event, RunState, RunStatus};

/// Replayed step history.
pub(super) struct History {
    /// Entries per step name, from `step_started` records.
    pub(super) attempts: BTreeMap<String, u32>,
    /// Latest finished outcome per step.
    pub(super) outcomes: BTreeMap<String, StepOutcome>,
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
            start,
            started,
        }
    }
}

/// What a replayed log says about how to proceed.
pub(super) enum Replay {
    /// The log holds `run_finished`; return the outcome untouched.
    Finished(StepOutcome),
    /// Continue from the replayed history.
    Resume(History),
}

/// The pipeline's entry step.
pub(super) fn entry(pipeline: &Pipeline) -> Route {
    pipeline.steps.first().map_or_else(
        || Route::Fail("pipeline has no steps".to_owned()),
        |step| Route::Step(step.name.clone()),
    )
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

/// Folds step history into attempt counts, outcomes, and the
/// continuation route.
fn history_from(state: &RunState, pipeline: &Pipeline) -> History {
    let mut attempts = BTreeMap::new();
    let mut outcomes = BTreeMap::new();
    for record in &state.steps {
        *attempts.entry(record.step.clone()).or_insert(0) += 1;
        if let Some(outcome) = record.outcome {
            outcomes.insert(record.step.clone(), outcome);
        }
    }
    History {
        attempts,
        outcomes,
        start: next_route(state, pipeline),
        started: true,
    }
}

/// Replays events into a resume decision.
pub(super) fn replay(events: Vec<Event>, pipeline: &Pipeline) -> Replay {
    let Some(state) = runlog::replay(events) else {
        return Replay::Resume(History::fresh(entry(pipeline), false));
    };
    if let RunStatus::Finished(outcome) = state.status {
        return Replay::Finished(outcome);
    }
    Replay::Resume(history_from(&state, pipeline))
}
