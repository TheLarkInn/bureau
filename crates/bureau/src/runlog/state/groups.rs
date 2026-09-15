//! Concurrent-group projection, unchanged by the independent factory projection.

use super::super::event::Event;
use super::super::group::{
    GroupFinishedData, GroupMemberCancelledData, GroupMemberFinishedData, GroupMemberStartedData,
    GroupStartedData,
};
use super::super::{GroupMemberRecord, GroupRecord};
use super::{RunState, StepRecord};

impl RunState {
    pub(super) fn group_active(&self, group: &str) -> bool {
        self.groups.contains_key(group)
            && self
                .steps
                .last()
                .is_some_and(|step| step.step == group && step.outcome.is_none())
    }

    pub(super) fn has_active_group(&self) -> bool {
        self.steps
            .last()
            .is_some_and(|step| self.group_active(&step.step))
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
        member.halted = data.halted;
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
        let Some(step) = steps.last_mut() else { return };
        step.outcome = Some(data.result.outcome);
        step.result = Some(data.result.clone());
        step.usage = Some(data.usage.clone());
        group.result = Some(data.result);
        group.usage = Some(data.usage);
        group.halted = data.halted;
    }
}
