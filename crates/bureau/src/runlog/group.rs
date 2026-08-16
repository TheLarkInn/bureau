//! Concurrent-group event payloads and builders.

use serde::{Deserialize, Serialize};

use crate::adapters::{Execution, Usage};
use crate::config::Completion;
use crate::contract::StepResult;

/// Payload of a `group_started` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupStartedData {
    /// Concurrent step name.
    pub group: String,
    /// Fixed member step names.
    pub members: Vec<String>,
    /// When unfinished members are cancelled.
    pub completion: Completion,
    /// Resolved positive member limit.
    pub max_concurrent: usize,
    /// Internal Git snapshot shared by every member.
    pub snapshot: String,
}

/// Payload of a `group_member_started` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMemberStartedData {
    /// Concurrent step name.
    pub group: String,
    /// Member step name.
    pub member: String,
    /// One-based member attempt number.
    pub attempt: u32,
}

/// Payload of a `group_member_finished` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMemberFinishedData {
    /// Concurrent step name.
    pub group: String,
    /// Member step name.
    pub member: String,
    /// Full scrubbed member result.
    pub result: StepResult,
    /// Adapter-owned member usage.
    pub usage: Usage,
}

/// Payload of a `group_member_cancelled` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMemberCancelledData {
    /// Concurrent step name.
    pub group: String,
    /// Member step name.
    pub member: String,
    /// Human-readable cancellation reason.
    pub reason: String,
}

/// Payload of a `group_finished` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupFinishedData {
    /// Concurrent step name.
    pub group: String,
    /// Full aggregate result.
    pub result: StepResult,
    /// Aggregate adapter usage.
    pub usage: Usage,
}

fn to_value<T: Serialize>(data: &T) -> serde_json::Value {
    serde_json::to_value(data).unwrap_or(serde_json::Value::Null)
}

/// Builds the `data` for a `group_started` event.
#[must_use]
pub fn group_started(
    group: &str,
    members: &[String],
    completion: Completion,
    max_concurrent: usize,
    snapshot: &str,
) -> serde_json::Value {
    to_value(&GroupStartedData {
        group: group.to_owned(),
        members: members.to_vec(),
        completion,
        max_concurrent,
        snapshot: snapshot.to_owned(),
    })
}

/// Builds the `data` for a `group_member_started` event.
#[must_use]
pub fn group_member_started(group: &str, member: &str, attempt: u32) -> serde_json::Value {
    to_value(&GroupMemberStartedData {
        group: group.to_owned(),
        member: member.to_owned(),
        attempt,
    })
}

/// Builds the `data` for a `group_member_finished` event.
#[must_use]
pub fn group_member_finished(
    group: &str,
    member: &str,
    execution: &Execution,
) -> serde_json::Value {
    to_value(&GroupMemberFinishedData {
        group: group.to_owned(),
        member: member.to_owned(),
        result: execution.result.clone(),
        usage: execution.usage.clone(),
    })
}

/// Builds the `data` for a `group_member_cancelled` event.
#[must_use]
pub fn group_member_cancelled(group: &str, member: &str, reason: &str) -> serde_json::Value {
    to_value(&GroupMemberCancelledData {
        group: group.to_owned(),
        member: member.to_owned(),
        reason: reason.to_owned(),
    })
}

/// Builds the `data` for a `group_finished` event.
#[must_use]
pub fn group_finished(group: &str, execution: &Execution) -> serde_json::Value {
    to_value(&GroupFinishedData {
        group: group.to_owned(),
        result: execution.result.clone(),
        usage: execution.usage.clone(),
    })
}
