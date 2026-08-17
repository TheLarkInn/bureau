//! One-line, message-ish event summaries — the shared display language
//! of `bureau show` and `bureau watch`.

use super::event::{
    BranchPushedData, CheckpointData, Event, EventKind, OutputData, PrCreatedData, RunFinishedData,
    RunStartedData, StepFinishedData, StepStartedData,
};
use super::group::{
    GroupFinishedData, GroupMemberCancelledData, GroupMemberFinishedData, GroupMemberStartedData,
    GroupStartedData,
};
use super::state::RunStatus;
use crate::contract::StepOutcome;

/// The kebab-case token for an outcome (its serde name).
#[must_use]
pub const fn outcome_name(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Success => "success",
        StepOutcome::Failure => "failure",
        StepOutcome::Blocked => "blocked",
        StepOutcome::NoWork => "no-work",
    }
}

/// The snake-case token for an event kind (its serde name).
#[must_use]
pub const fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::RunStarted => "run_started",
        EventKind::StepStarted => "step_started",
        EventKind::Output => "output",
        EventKind::StepFinished => "step_finished",
        EventKind::GroupStarted => "group_started",
        EventKind::GroupMemberStarted => "group_member_started",
        EventKind::GroupMemberFinished => "group_member_finished",
        EventKind::GroupMemberCancelled => "group_member_cancelled",
        EventKind::GroupFinished => "group_finished",
        EventKind::Checkpoint => "checkpoint",
        EventKind::BranchPushed => "branch_pushed",
        EventKind::PrCreated => "pr_created",
        EventKind::RunFinished => "run_finished",
    }
}

/// `running` or `finished(<outcome>)`.
#[must_use]
pub fn status_text(status: &RunStatus) -> String {
    match status {
        RunStatus::Running => "running".to_owned(),
        RunStatus::Finished(outcome) => format!("finished({})", outcome_name(*outcome)),
    }
}

/// Deserializes an event payload; a mismatch displays as an empty gist.
fn payload<T: serde::de::DeserializeOwned>(event: &Event) -> Option<T> {
    serde_json::from_value(event.data.clone()).ok()
}

/// `run_started`: the assignment and item.
fn started_gist(data: RunStartedData) -> String {
    let item = data.item.unwrap_or_else(|| "none".to_owned());
    format!("assignment={} item={item}", data.assignment)
}

/// `step_finished`: the step and its outcome.
fn step_gist(data: &StepFinishedData) -> String {
    format!("{} -> {}", data.step, outcome_name(data.outcome))
}

/// `output`: the stream and a trimmed chunk.
fn output_gist(data: &OutputData) -> String {
    format!("{}: {}", data.stream, data.data.trim_end())
}

fn group_gist(event: &Event) -> String {
    match event.kind {
        EventKind::GroupStarted => {
            payload::<GroupStartedData>(event).map_or_else(String::new, |data| data.group)
        }
        EventKind::GroupMemberStarted => payload::<GroupMemberStartedData>(event)
            .map_or_else(String::new, |data| {
                format!("{}:{}", data.group, data.member)
            }),
        EventKind::GroupMemberFinished => payload::<GroupMemberFinishedData>(event)
            .map_or_else(String::new, |data| {
                format!("{}:{}", data.group, data.member)
            }),
        EventKind::GroupMemberCancelled => payload::<GroupMemberCancelledData>(event)
            .map_or_else(String::new, |data| {
                format!("{}:{}", data.group, data.member)
            }),
        EventKind::GroupFinished => {
            payload::<GroupFinishedData>(event).map_or_else(String::new, |data| data.group)
        }
        _ => String::new(),
    }
}

fn durable_gist(event: &Event) -> String {
    match event.kind {
        EventKind::Checkpoint => {
            payload::<CheckpointData>(event).map_or_else(String::new, |data| data.commit)
        }
        EventKind::BranchPushed => payload::<BranchPushedData>(event)
            .map_or_else(String::new, |data| {
                format!("{} {}", data.branch, data.commit)
            }),
        EventKind::PrCreated => {
            payload::<PrCreatedData>(event).map_or_else(String::new, |data| data.pr.url)
        }
        _ => String::new(),
    }
}

/// A one-line, message-ish summary of an event's payload.
#[must_use]
pub fn gist(event: &Event) -> String {
    match event.kind {
        EventKind::RunStarted => {
            payload::<RunStartedData>(event).map_or_else(String::new, started_gist)
        }
        EventKind::StepStarted => {
            payload::<StepStartedData>(event).map_or_else(String::new, |d| d.step)
        }
        EventKind::StepFinished => {
            payload::<StepFinishedData>(event).map_or_else(String::new, |d| step_gist(&d))
        }
        EventKind::Output => {
            payload::<OutputData>(event).map_or_else(String::new, |d| output_gist(&d))
        }
        EventKind::GroupStarted
        | EventKind::GroupMemberStarted
        | EventKind::GroupMemberFinished
        | EventKind::GroupMemberCancelled
        | EventKind::GroupFinished => group_gist(event),
        EventKind::Checkpoint | EventKind::BranchPushed | EventKind::PrCreated => {
            durable_gist(event)
        }
        EventKind::RunFinished => payload::<RunFinishedData>(event)
            .map_or_else(String::new, |d| outcome_name(d.outcome).to_owned()),
    }
}
