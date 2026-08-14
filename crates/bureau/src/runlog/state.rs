//! Run state, derived by replaying the event log. `state.json` is only a
//! cache of this; the events are the source of truth (DESIGN.md layer 3).

use serde::{Deserialize, Serialize};

use super::event::{
    Event, EventKind, RunFinishedData, RunStartedData, StepFinishedData, StepStartedData,
};
use crate::contract::StepOutcome;

/// Where a run stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "outcome")]
pub enum RunStatus {
    /// The run has not finished.
    Running,
    /// The run finished with this outcome.
    Finished(StepOutcome),
}

/// One step's record within a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    /// Step name within the pipeline.
    pub step: String,
    /// Set when the step finishes.
    pub outcome: Option<StepOutcome>,
}

/// Everything the run log implies about a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunState {
    /// The run's id.
    pub run_id: String,
    /// The assignment the run belongs to.
    pub assignment: String,
    /// Steps in start order.
    pub steps: Vec<StepRecord>,
    /// Where the run stands.
    pub status: RunStatus,
}

impl RunState {
    fn from_event(event: &Event) -> Option<Self> {
        if event.kind != EventKind::RunStarted {
            return None;
        }
        let data = serde_json::from_value::<RunStartedData>(event.data.clone()).ok()?;
        Some(Self {
            run_id: data.run_id,
            assignment: data.assignment,
            steps: Vec::new(),
            status: RunStatus::Running,
        })
    }

    /// Folds one event into the state.
    pub fn apply(&mut self, event: &Event) {
        match event.kind {
            EventKind::RunStarted => {}
            EventKind::StepStarted => self.start_step(event),
            EventKind::StepFinished => self.finish_step(event),
            EventKind::RunFinished => self.finish_run(event),
        }
    }

    fn start_step(&mut self, event: &Event) {
        if let Ok(data) = serde_json::from_value::<StepStartedData>(event.data.clone()) {
            self.steps.push(StepRecord {
                step: data.step,
                outcome: None,
            });
        }
    }

    fn finish_step(&mut self, event: &Event) {
        let Ok(data) = serde_json::from_value::<StepFinishedData>(event.data.clone()) else {
            return;
        };
        if let Some(record) = self.steps.last_mut() {
            record.outcome = Some(data.outcome);
        }
    }

    fn finish_run(&mut self, event: &Event) {
        if let Ok(data) = serde_json::from_value::<RunFinishedData>(event.data.clone()) {
            self.status = RunStatus::Finished(data.outcome);
        }
    }
}

/// Rebuilds run state from the event log — the only source of truth.
///
/// Returns `None` when the log has no `run_started` event.
#[must_use]
pub fn replay(events: impl IntoIterator<Item = Event>) -> Option<RunState> {
    let mut iter = events.into_iter();
    let mut state = iter.by_ref().find_map(|e| RunState::from_event(&e))?;
    for event in iter {
        state.apply(&event);
    }
    Some(state)
}
