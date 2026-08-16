//! Run state, derived by replaying the event log. `state.json` is only a
//! cache of this; the events are the source of truth (DESIGN.md layer 3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::RunSnapshot;
use super::event::{
    BranchPushedData, CheckpointData, Event, EventKind, PrCreatedData, RunFinishedData,
    RunStartedData, StepFinishedData, StepStartedData,
};
use super::group_state::GroupRecord;
use crate::adapters::Usage;
use crate::contract::{StepOutcome, StepResult};
use crate::forge::Pr;

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    /// Step name within the pipeline.
    pub step: String,
    /// Set when the step finishes.
    pub outcome: Option<StepOutcome>,
    /// Full result when the step finished.
    pub result: Option<StepResult>,
    /// Adapter-owned usage when the step finished.
    pub usage: Option<Usage>,
}

/// Everything the run log implies about a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    /// The run's id.
    pub run_id: String,
    /// The assignment the run belongs to.
    pub assignment: String,
    /// Wall-clock start from the first run event.
    pub started_at_ms: u64,
    /// Immutable plan snapshot when the engine wrote one.
    pub snapshot: Option<RunSnapshot>,
    /// Steps in start order.
    pub steps: Vec<StepRecord>,
    /// Concurrent groups keyed deterministically by step name.
    #[serde(default)]
    pub groups: BTreeMap<String, GroupRecord>,
    /// Where the run stands.
    pub status: RunStatus,
    /// Latest durable branch checkpoint.
    pub checkpoint: Option<String>,
    /// Run branch base before step changes.
    pub base_commit: Option<String>,
    /// Exact pushed commit.
    pub pushed_commit: Option<String>,
    /// Created or adopted PR.
    pub pr: Option<Pr>,
    /// Complete terminal payload.
    pub finished: Option<RunFinishedData>,
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
            started_at_ms: event.at_ms,
            snapshot: data.snapshot,
            steps: Vec::new(),
            groups: BTreeMap::new(),
            status: RunStatus::Running,
            checkpoint: None,
            base_commit: None,
            pushed_commit: None,
            pr: None,
            finished: None,
        })
    }

    /// Folds one event into the state.
    pub fn apply(&mut self, event: &Event) {
        match event.kind {
            EventKind::RunStarted | EventKind::Output => {}
            EventKind::StepStarted => self.start_step(event),
            EventKind::StepFinished => self.finish_step(event),
            EventKind::GroupStarted => self.start_group(event),
            EventKind::GroupMemberStarted => self.start_group_member(event),
            EventKind::GroupMemberFinished => self.finish_group_member(event),
            EventKind::GroupMemberCancelled => self.cancel_group_member(event),
            EventKind::GroupFinished => self.finish_group(event),
            EventKind::Checkpoint => self.checkpoint(event),
            EventKind::BranchPushed => self.branch_pushed(event),
            EventKind::PrCreated => self.pr_created(event),
            EventKind::RunFinished => self.finish_run(event),
        }
    }

    fn start_step(&mut self, event: &Event) {
        if self.has_active_group() {
            return;
        }
        if let Ok(data) = serde_json::from_value::<StepStartedData>(event.data.clone()) {
            self.steps.push(StepRecord {
                step: data.step,
                outcome: None,
                result: None,
                usage: None,
            });
        }
    }

    fn finish_step(&mut self, event: &Event) {
        let Ok(data) = serde_json::from_value::<StepFinishedData>(event.data.clone()) else {
            return;
        };
        if self.group_active(&data.step) {
            return;
        }
        if let Some(record) = self.steps.last_mut() {
            if record.step != data.step || record.outcome.is_some() {
                return;
            }
            record.outcome = Some(data.outcome);
            record.result = data.result;
            record.usage = data.usage;
        }
    }

    fn checkpoint(&mut self, event: &Event) {
        if let Ok(data) = serde_json::from_value::<CheckpointData>(event.data.clone()) {
            self.base_commit = Some(data.base_commit);
            self.checkpoint = Some(data.commit);
        }
    }

    fn branch_pushed(&mut self, event: &Event) {
        if let Ok(data) = serde_json::from_value::<BranchPushedData>(event.data.clone()) {
            self.pushed_commit = Some(data.commit);
        }
    }

    fn pr_created(&mut self, event: &Event) {
        if let Ok(data) = serde_json::from_value::<PrCreatedData>(event.data.clone()) {
            self.pr = Some(data.pr);
        }
    }

    fn finish_run(&mut self, event: &Event) {
        if let Ok(data) = serde_json::from_value::<RunFinishedData>(event.data.clone()) {
            self.status = RunStatus::Finished(data.outcome);
            self.finished = Some(data);
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
