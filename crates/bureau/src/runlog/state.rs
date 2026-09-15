//! Run state, derived by replaying the event log. `state.json` is only a
//! cache of this; the events are the source of truth (DESIGN.md layer 3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::event::{
    BranchPushedData, CheckpointData, Event, EventKind, PrCreatedData, RunFinishedData,
    RunStartedData, StepFinishedData, StepStartedData,
};
use super::{GroupRecord, RunSnapshot};
use crate::adapters::Usage;
use crate::contract::{StepOutcome, StepResult};
use crate::forge::Pr;

mod groups;

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

fn factory_records_empty(records: &super::copilot_factory::Records) -> bool {
    records.0.is_empty()
}

fn started(event: &Event) -> Option<RunStartedData> {
    if event.kind != EventKind::RunStarted {
        return None;
    }
    serde_json::from_value(event.data.clone()).ok()
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
    /// Local runtime identities; never cloud automation/task records.
    #[serde(default, skip_serializing_if = "factory_records_empty")]
    pub copilot_factories: super::copilot_factory::Records,
    /// A corrupt local-factory event remains visible and blocks trustworthy projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot_factory_error: Option<String>,
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
    pub(super) fn from_event(event: &Event) -> Option<Self> {
        let data = started(event)?;
        Some(Self {
            run_id: data.run_id,
            assignment: data.assignment,
            started_at_ms: event.at_ms,
            snapshot: data.snapshot,
            steps: Vec::new(),
            groups: BTreeMap::new(),
            copilot_factories: super::copilot_factory::Records::default(),
            copilot_factory_error: None,
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
            EventKind::RunStarted | EventKind::Output | EventKind::GitHubCloud => {}
            EventKind::CopilotFactory => self.factory(event),
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

    fn factory(&mut self, event: &Event) {
        if self.copilot_factory_error.is_some() {
            return;
        }
        let mut next = self.copilot_factories.clone();
        let applied = serde_json::from_value(event.data.clone())
            .map_err(|error| error.to_string())
            .and_then(|data| next.apply(data));
        match applied {
            Ok(()) => self.copilot_factories = next,
            Err(error) => {
                self.copilot_factory_error = Some(format!("event {}: {error}", event.seq));
            }
        }
    }
}
