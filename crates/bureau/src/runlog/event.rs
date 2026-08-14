//! Run-log events and their typed payloads (DESIGN.md layer 3).

use serde::{Deserialize, Serialize};

use crate::contract::StepOutcome;

/// The kind of a run-log event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A run began for an assignment.
    RunStarted,
    /// A pipeline step began.
    StepStarted,
    /// A pipeline step finished with an outcome.
    StepFinished,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStartedData {
    /// The run's id.
    pub run_id: String,
    /// The assignment the run belongs to.
    pub assignment: String,
}

/// Payload of a `step_started` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepStartedData {
    /// Step name within the pipeline.
    pub step: String,
}

/// Payload of a `step_finished` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepFinishedData {
    /// Step name within the pipeline.
    pub step: String,
    /// What the step concluded.
    pub outcome: StepOutcome,
}

/// Payload of a `run_finished` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFinishedData {
    /// What the run concluded.
    pub outcome: StepOutcome,
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
    })
}

/// Builds the `data` for a `run_finished` event.
#[must_use]
pub fn run_finished(outcome: StepOutcome) -> serde_json::Value {
    to_value(&RunFinishedData { outcome })
}
