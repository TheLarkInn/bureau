//! Concurrent-group record construction and validation.

use std::collections::BTreeMap;

use super::group::GroupStartedData;
use super::{GroupMemberRecord, GroupRecord};

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

impl GroupRecord {
    pub(super) fn started(data: &GroupStartedData) -> Option<Self> {
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
            halted: false,
        })
    }
}

impl GroupMemberRecord {
    pub(super) const fn is_terminal(&self) -> bool {
        self.result.is_some() || self.cancellation_reason.is_some()
    }
}
