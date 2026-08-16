//! Concurrent-group state projection.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::event::Event;
use super::group::{
    GroupFinishedData, GroupMemberCancelledData, GroupMemberFinishedData, GroupMemberStartedData,
    GroupStartedData,
};
use super::state::{RunState, StepRecord};
use crate::adapters::Usage;
use crate::config::Completion;
use crate::contract::StepResult;

/// Durable state for one concurrent group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupRecord {
    /// Members keyed deterministically by step name.
    pub members: BTreeMap<String, GroupMemberRecord>,
    /// When unfinished members are cancelled.
    pub completion: Completion,
    /// Resolved positive member limit.
    pub max_concurrent: usize,
    /// Internal Git snapshot shared by every member.
    pub snapshot: String,
    /// Aggregate result after the group finishes.
    pub result: Option<StepResult>,
    /// Aggregate usage after the group finishes.
    pub usage: Option<Usage>,
}

/// Durable state for one concurrent group member.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupMemberRecord {
    /// Number of attempts that began.
    pub attempts: u32,
    /// Full result after the member finishes.
    pub result: Option<StepResult>,
    /// Adapter-owned usage after the member finishes.
    pub usage: Option<Usage>,
    /// Why the unfinished member was cancelled.
    pub cancellation_reason: Option<String>,
}

impl RunState {
    pub(super) fn start_group(&mut self, event: &Event) {
        let Ok(data) = serde_json::from_value::<GroupStartedData>(event.data.clone()) else {
            return;
        };
        let Some(group) = GroupRecord::started(&data) else {
            return;
        };
        if self.steps.last().is_some_and(|step| step.outcome.is_none()) {
            return;
        }
        self.groups.insert(data.group.clone(), group);
        self.steps.push(StepRecord {
            step: data.group,
            outcome: None,
            result: None,
            usage: None,
        });
    }

    pub(super) fn start_group_member(&mut self, event: &Event) {
        let Ok(data) = serde_json::from_value::<GroupMemberStartedData>(event.data.clone()) else {
            return;
        };
        let Some(member) = self.active_group_member_mut(&data.group, &data.member) else {
            return;
        };
        let Some(attempt) = member.attempts.checked_add(1) else {
            return;
        };
        if member.is_terminal() || data.attempt != attempt {
            return;
        }
        member.attempts = attempt;
    }

    pub(super) fn finish_group_member(&mut self, event: &Event) {
        let Ok(data) = serde_json::from_value::<GroupMemberFinishedData>(event.data.clone()) else {
            return;
        };
        let Some(member) = self.active_group_member_mut(&data.group, &data.member) else {
            return;
        };
        if member.attempts == 0 || member.is_terminal() {
            return;
        }
        member.result = Some(data.result);
        member.usage = Some(data.usage);
    }

    pub(super) fn cancel_group_member(&mut self, event: &Event) {
        let Ok(data) = serde_json::from_value::<GroupMemberCancelledData>(event.data.clone())
        else {
            return;
        };
        let Some(member) = self.active_group_member_mut(&data.group, &data.member) else {
            return;
        };
        if member.is_terminal() || data.reason.trim().is_empty() {
            return;
        }
        member.cancellation_reason = Some(data.reason);
    }

    pub(super) fn finish_group(&mut self, event: &Event) {
        let Ok(data) = serde_json::from_value::<GroupFinishedData>(event.data.clone()) else {
            return;
        };
        if !self.group_can_finish(&data.group) {
            return;
        }
        let (groups, steps) = (&mut self.groups, &mut self.steps);
        let Some(group) = groups.get_mut(&data.group) else {
            return;
        };
        let Some(step) = steps.last_mut() else {
            return;
        };
        step.outcome = Some(data.result.outcome);
        step.result = Some(data.result.clone());
        step.usage = Some(data.usage.clone());
        group.result = Some(data.result);
        group.usage = Some(data.usage);
    }

    pub(super) fn has_active_group(&self) -> bool {
        self.steps
            .last()
            .is_some_and(|step| self.group_active(&step.step))
    }

    pub(super) fn group_active(&self, group: &str) -> bool {
        self.groups.contains_key(group)
            && self
                .steps
                .last()
                .is_some_and(|step| step.step == group && step.outcome.is_none())
    }

    fn active_group_member_mut(
        &mut self,
        group: &str,
        member: &str,
    ) -> Option<&mut GroupMemberRecord> {
        if !self.group_active(group) {
            return None;
        }
        let group = self.groups.get_mut(group)?;
        if group.result.is_some() {
            return None;
        }
        group.members.get_mut(member)
    }

    fn group_can_finish(&self, group: &str) -> bool {
        if !self.group_active(group) {
            return false;
        }
        self.groups.get(group).is_some_and(|record| {
            record.result.is_none() && record.members.values().all(GroupMemberRecord::is_terminal)
        })
    }
}

impl GroupRecord {
    fn started(data: &GroupStartedData) -> Option<Self> {
        let members = member_records(&data.members)?;
        if !valid_start(data, members.len()) {
            return None;
        }
        Some(Self {
            members,
            completion: data.completion,
            max_concurrent: data.max_concurrent,
            snapshot: data.snapshot.clone(),
            result: None,
            usage: None,
        })
    }
}

impl GroupMemberRecord {
    const fn is_terminal(&self) -> bool {
        self.result.is_some() || self.cancellation_reason.is_some()
    }
}

fn member_records(names: &[String]) -> Option<BTreeMap<String, GroupMemberRecord>> {
    if names.is_empty() || names.iter().any(|name| name.trim().is_empty()) {
        return None;
    }
    let records = names
        .iter()
        .cloned()
        .map(|name| (name, GroupMemberRecord::default()))
        .collect::<BTreeMap<_, _>>();
    (records.len() == names.len()).then_some(records)
}

fn valid_start(data: &GroupStartedData, member_count: usize) -> bool {
    !data.group.trim().is_empty()
        && !data.snapshot.trim().is_empty()
        && data.max_concurrent > 0
        && data.max_concurrent <= member_count
}
