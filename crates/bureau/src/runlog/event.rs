//! Run-log events and their typed payloads (DESIGN.md layer 3).

use serde::{Deserialize, Serialize};

use crate::adapters::{Execution, Usage};
use crate::contract::{StepOutcome, StepResult};
use crate::forge::Pr;

use super::RunSnapshot;

/// The kind of a run-log event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A run began for an assignment.
    RunStarted,
    /// A pipeline step began.
    StepStarted,
    /// Captured (scrubbed) subprocess output, streamed as it arrives.
    Output,
    /// A pipeline step finished with an outcome.
    StepFinished,
    /// The run branch was checkpointed after a step.
    Checkpoint,
    /// The final branch commit was pushed.
    BranchPushed,
    /// A PR was created or adopted.
    PrCreated,
    /// The run finished with an outcome.
    RunFinished,
}

/// One append-only, sequence-numbered run-log record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Position in the log, starting at 0.
    pub seq: u64,
    /// Wall time, milliseconds since the Unix epoch.
    pub at_ms: u64,
    /// What happened.
    pub kind: EventKind,
    /// Kind-specific payload.
    pub data: serde_json::Value,
}

/// Payload of a `run_started` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStartedData {
    /// The run's id.
    pub run_id: String,
    /// The assignment the run belongs to.
    pub assignment: String,
    /// The work item being run, when the run is for one. `bureau retry`
    /// reads this.
    #[serde(default)]
    pub item: Option<String>,
    /// Immutable plan snapshot for automatic restart.
    #[serde(default)]
    pub snapshot: Option<RunSnapshot>,
}

/// Payload of an `output` event: one scrubbed chunk of a stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputData {
    /// The step whose process produced it, when known.
    pub step: Option<String>,
    /// `stdout` or `stderr`.
    pub stream: String,
    /// The scrubbed bytes (UTF-8).
    pub data: String,
}

/// Payload of a `step_started` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepStartedData {
    /// Step name within the pipeline.
    pub step: String,
}

/// Payload of a `step_finished` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepFinishedData {
    /// Step name within the pipeline.
    pub step: String,
    /// What the step concluded.
    pub outcome: StepOutcome,
    /// Full scrubbed result for explicit downstream data flow and resume.
    #[serde(default)]
    pub result: Option<StepResult>,
    /// Adapter-owned usage.
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Payload of a `run_finished` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunFinishedData {
    /// What the run concluded.
    pub outcome: StepOutcome,
    /// Human-readable terminal detail.
    #[serde(default)]
    pub message: String,
    /// Adapter-measured total cost.
    #[serde(default)]
    pub cost_usd: f64,
    /// PR created/adopted by this run.
    #[serde(default)]
    pub pr: Option<Pr>,
    /// Dedup disposition projected after the terminal event.
    #[serde(default)]
    pub disposition: Option<TerminalDisposition>,
}

/// State projection implied by a terminal run event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalDisposition {
    /// A PR exists for this content.
    Proposed,
    /// The run settled without a PR.
    NoChange,
}

impl TerminalDisposition {
    /// One terminal outcome's durable dedup policy.
    #[must_use]
    pub const fn for_outcome(outcome: StepOutcome, has_pr: bool) -> Option<Self> {
        if has_pr {
            return Some(Self::Proposed);
        }
        match outcome {
            StepOutcome::Failure => None,
            StepOutcome::Success | StepOutcome::Blocked | StepOutcome::NoWork => {
                Some(Self::NoChange)
            }
        }
    }
}

/// Durable branch checkpoint after one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointData {
    /// Step whose work was checkpointed.
    pub step: String,
    /// Run branch base before any step changes.
    pub base_commit: String,
    /// Exact Git commit.
    pub commit: String,
}

/// Final pushed branch state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchPushedData {
    /// Head branch.
    pub branch: String,
    /// Exact pushed commit.
    pub commit: String,
}

/// Created or observed PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrCreatedData {
    /// Exact PR identity.
    pub pr: Pr,
    /// Pushed commit the PR represents.
    pub commit: String,
}

fn to_value<T: Serialize>(data: &T) -> serde_json::Value {
    serde_json::to_value(data).unwrap_or(serde_json::Value::Null)
}

/// Builds the `data` for a `run_started` event.
#[must_use]
pub fn run_started(run_id: &str, assignment: &str) -> serde_json::Value {
    to_value(&RunStartedData {
        run_id: run_id.to_owned(),
        assignment: assignment.to_owned(),
        item: None,
        snapshot: None,
    })
}

/// Builds the `data` for a `run_started` event tied to a work item.
#[must_use]
pub fn run_started_for_item(run_id: &str, assignment: &str, item: &str) -> serde_json::Value {
    to_value(&RunStartedData {
        run_id: run_id.to_owned(),
        assignment: assignment.to_owned(),
        item: Some(item.to_owned()),
        snapshot: None,
    })
}

/// Builds a `run_started` payload with the complete immutable plan.
#[must_use]
pub fn run_started_snapshot(snapshot: &RunSnapshot) -> serde_json::Value {
    to_value(&RunStartedData {
        run_id: snapshot.run_id.clone(),
        assignment: snapshot.assignment.name.clone(),
        item: Some(snapshot.item.external_id.clone()),
        snapshot: Some(snapshot.clone()),
    })
}

/// Builds the `data` for an `output` event.
#[must_use]
pub fn output(step: Option<&str>, stream: &str, data: &str) -> serde_json::Value {
    to_value(&OutputData {
        step: step.map(str::to_owned),
        stream: stream.to_owned(),
        data: data.to_owned(),
    })
}

/// Builds the `data` for a `step_started` event.
#[must_use]
pub fn step_started(step: &str) -> serde_json::Value {
    to_value(&StepStartedData {
        step: step.to_owned(),
    })
}

/// Builds the `data` for a `step_finished` event.
#[must_use]
pub fn step_finished(step: &str, outcome: StepOutcome) -> serde_json::Value {
    to_value(&StepFinishedData {
        step: step.to_owned(),
        outcome,
        result: None,
        usage: None,
    })
}

/// Builds full durable step data.
#[must_use]
pub fn step_finished_full(step: &str, execution: &Execution) -> serde_json::Value {
    to_value(&StepFinishedData {
        step: step.to_owned(),
        outcome: execution.result.outcome,
        result: Some(execution.result.clone()),
        usage: Some(execution.usage.clone()),
    })
}

/// Builds the legacy-minimal `data` for a `run_finished` event.
#[must_use]
pub fn run_finished(outcome: StepOutcome) -> serde_json::Value {
    run_finished_full(outcome, "", 0.0, None, None)
}

/// Builds complete terminal data.
#[must_use]
pub fn run_finished_full(
    outcome: StepOutcome,
    message: &str,
    cost_usd: f64,
    pr: Option<&Pr>,
    disposition: Option<TerminalDisposition>,
) -> serde_json::Value {
    to_value(&RunFinishedData {
        outcome,
        message: message.to_owned(),
        cost_usd,
        pr: pr.cloned(),
        disposition,
    })
}

/// Builds a branch checkpoint payload.
#[must_use]
pub fn checkpoint(step: &str, base_commit: &str, commit: &str) -> serde_json::Value {
    to_value(&CheckpointData {
        step: step.to_owned(),
        base_commit: base_commit.to_owned(),
        commit: commit.to_owned(),
    })
}

/// Builds a branch-pushed payload.
#[must_use]
pub fn branch_pushed(branch: &str, commit: &str) -> serde_json::Value {
    to_value(&BranchPushedData {
        branch: branch.to_owned(),
        commit: commit.to_owned(),
    })
}

/// Builds a PR-created/adopted payload.
#[must_use]
pub fn pr_created(pr: &Pr, commit: &str) -> serde_json::Value {
    to_value(&PrCreatedData {
        pr: pr.clone(),
        commit: commit.to_owned(),
    })
}
