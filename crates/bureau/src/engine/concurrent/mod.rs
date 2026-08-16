//! Static concurrent evidence-group execution and partial replay.

mod member;
mod result;
mod schedule;
mod snapshot;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::adapters::Execution;
use crate::config::{Completion, StepDef};
use crate::runlog::{self, EventKind, GroupRecord};

use super::machine::{self, RunCtx, WtCtx};

struct State {
    snapshot: String,
    completion: Completion,
    limit: usize,
    results: BTreeMap<String, Execution>,
    cancelled: BTreeMap<String, String>,
    attempts: BTreeMap<String, u32>,
}

pub(super) async fn run(ctx: &mut RunCtx, wt: &WtCtx, group: &StepDef) -> Execution {
    let root = group_root(ctx, group);
    let mut state = state(ctx, wt, group).await;
    let members = pending_members(ctx, group, &mut state);
    let mut schedule = schedule::Schedule::new(members, state.limit, state.completion);
    drive(ctx, wt, group, &root, &mut state, &mut schedule).await;
    let execution = result::aggregate(&state.results, &state.cancelled);
    let data = runlog::group_finished(&group.name, &execution);
    machine::append(ctx, EventKind::GroupFinished, data);
    execution
}

async fn state(ctx: &mut RunCtx, wt: &WtCtx, group: &StepDef) -> State {
    if let Some(record) = ctx
        .groups
        .get(&group.name)
        .filter(|record| record.result.is_none())
    {
        return resumed(record);
    }
    fresh_state(ctx, wt, group).await
}

async fn fresh_state(ctx: &mut RunCtx, wt: &WtCtx, group: &StepDef) -> State {
    let root = machine::run_dir(ctx);
    let snapshot = snapshot::create(wt.worktree.path(), &root, &member::safe(&group.name))
        .await
        .unwrap_or_else(|error| format!("invalid-snapshot:{error}"));
    let limit = resolved_limit(group);
    let completion = group.completion.unwrap_or(Completion::All);
    let data = runlog::group_started(&group.name, &group.steps, completion, limit, &snapshot);
    machine::append(ctx, EventKind::GroupStarted, data);
    ctx.begin_attempt(&group.name);
    State {
        snapshot,
        completion,
        limit,
        results: BTreeMap::new(),
        cancelled: BTreeMap::new(),
        attempts: BTreeMap::new(),
    }
}

fn resumed(record: &GroupRecord) -> State {
    State {
        snapshot: record.snapshot.clone(),
        completion: record.completion,
        limit: record.max_concurrent,
        results: resumed_results(record),
        cancelled: resumed_cancellations(record),
        attempts: record
            .members
            .iter()
            .map(|(name, member)| (name.clone(), member.attempts))
            .collect(),
    }
}

fn resumed_results(record: &GroupRecord) -> BTreeMap<String, Execution> {
    record
        .members
        .iter()
        .filter_map(|(name, member)| {
            Some((
                name.clone(),
                Execution::new(member.result.clone()?, member.usage.clone()?),
            ))
        })
        .collect()
}

fn resumed_cancellations(record: &GroupRecord) -> BTreeMap<String, String> {
    record
        .members
        .iter()
        .filter_map(|(name, member)| Some((name.clone(), member.cancellation_reason.clone()?)))
        .collect()
}

fn pending_members(ctx: &RunCtx, group: &StepDef, state: &mut State) -> Vec<StepDef> {
    let mut pending = Vec::new();
    for name in &group.steps {
        if state.results.contains_key(name) || state.cancelled.contains_key(name) {
            continue;
        }
        let Some(step) = ctx
            .plan
            .pipeline
            .steps
            .iter()
            .find(|step| step.name == *name)
        else {
            continue;
        };
        if retryable(ctx, group, state, step).is_some() {
            pending.push(step.clone());
        }
    }
    pending
}

fn retryable<'a>(
    ctx: &RunCtx,
    group: &StepDef,
    state: &mut State,
    step: &'a StepDef,
) -> Option<&'a StepDef> {
    let attempts = state.attempts.get(&step.name).copied().unwrap_or(0);
    if attempts < step.max_attempts {
        return Some(step);
    }
    let reason = format!("member `{}` exceeded max attempts", step.name);
    state.cancelled.insert(step.name.clone(), reason.clone());
    let data = runlog::group_member_cancelled(&group.name, &step.name, &reason);
    machine::append(ctx, EventKind::GroupMemberCancelled, data);
    None
}

async fn drive(
    ctx: &RunCtx,
    wt: &WtCtx,
    group: &StepDef,
    root: &Path,
    state: &mut State,
    schedule: &mut schedule::Schedule,
) {
    while !schedule.is_finished() {
        start_available(ctx, wt, group, root, state, schedule);
        let Some((name, execution)) = schedule.next().await else {
            break;
        };
        finish_member(ctx, group, state, schedule, &name, execution);
    }
}

fn start_available(
    ctx: &RunCtx,
    wt: &WtCtx,
    group: &StepDef,
    root: &Path,
    state: &mut State,
    schedule: &mut schedule::Schedule,
) {
    for name in schedule.fill(ctx, &wt.mirror, root, &state.snapshot) {
        let attempt = state.attempts.entry(name.clone()).or_insert(0);
        *attempt = attempt.saturating_add(1);
        let data = runlog::group_member_started(&group.name, &name, *attempt);
        machine::append(ctx, EventKind::GroupMemberStarted, data);
    }
}

fn finish_member(
    ctx: &RunCtx,
    group: &StepDef,
    state: &mut State,
    schedule: &mut schedule::Schedule,
    name: &str,
    execution: Execution,
) {
    if schedule.was_cancelled(name) {
        return;
    }
    let outcome = execution.result.outcome;
    let data = runlog::group_member_finished(&group.name, name, &execution);
    machine::append(ctx, EventKind::GroupMemberFinished, data);
    state.results.insert(name.to_owned(), execution);
    let reason = format!(
        "group `{}` stopped after member `{name}` failed",
        group.name
    );
    for cancelled in schedule.cancel_after_failure(outcome, &reason) {
        state.cancelled.insert(cancelled.clone(), reason.clone());
        let data = runlog::group_member_cancelled(&group.name, &cancelled, &reason);
        machine::append(ctx, EventKind::GroupMemberCancelled, data);
    }
}

fn resolved_limit(group: &StepDef) -> usize {
    group
        .max_concurrent
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(group.steps.len())
}

fn group_root(ctx: &RunCtx, group: &StepDef) -> PathBuf {
    machine::run_dir(ctx)
        .join("concurrent")
        .join(member::safe(&group.name))
}
