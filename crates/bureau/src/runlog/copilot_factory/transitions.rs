//! Native acceptance is correlated to a durable dispatch, never to progress events.

use std::num::NonZeroU64;

use super::{Data, Intent, Operation, Record, Records, RuntimePurpose};
use crate::adapters::copilot_factory::types::{FactoryRunResult, FactoryRunSummary};

pub(super) fn intent(records: &Records, intent: &Intent) -> Result<(), String> {
    if records.0.contains_key(&intent.session_id)
        || records.0.values().any(|record| {
            record.intent.step == intent.step && record.intent.step_attempt == intent.step_attempt
        })
    {
        return Err("factory session or Bureau step attempt already has a durable intent".into());
    }
    if intent.step_attempt == 0
        || intent.step != intent.request.step
        || intent.workspace.directory != intent.request.worktree
        || intent.context.worktree != intent.workspace.directory
    {
        return Err(
            "factory intent is not bound to its original step attempt and workspace".into(),
        );
    }
    if intent.request.schema != crate::contract::SCHEMA_VERSION {
        return Err("factory intent has an unsupported step request schema".into());
    }
    Ok(())
}

fn admitted(record: &Record) -> Result<(), String> {
    if record.active_runtime != Some(RuntimePurpose::Execution)
        || record.runtime_session != Some(RuntimePurpose::Execution)
    {
        return Err("factory admission requires an acknowledged execution session".into());
    }
    if record.indeterminate.is_some() {
        return Err("indeterminate factory execution cannot be admitted again".into());
    }
    Ok(())
}

fn controlled(record: &Record) -> Result<(), String> {
    if record.active_runtime.is_none()
        || record.runtime_session != record.active_runtime
        || record.run_id.is_none()
    {
        return Err("factory control requires an acknowledged session and known native ID".into());
    }
    Ok(())
}

fn admission_dispatch(record: &Record, operation: Operation) -> Result<(), String> {
    admitted(record)?;
    if record.pending_admission.is_some() {
        return Err("factory admission is already outstanding".into());
    }
    match operation {
        Operation::Start if record.run_id.is_some() || record.dispatched.is_some() => {
            Err("fresh factory admission may only be dispatched once".into())
        }
        Operation::Resume if record.run_id.is_none() => {
            Err("factory control requires its correlated native run ID".into())
        }
        _ => Ok(()),
    }
}

fn dispatch(record: &Record, operation: Operation) -> Result<(), String> {
    if matches!(operation, Operation::Pause | Operation::Cancel) {
        controlled(record)
    } else {
        admission_dispatch(record, operation)
    }
}

fn acceptance(record: &Record, run_id: &str, attempt: NonZeroU64) -> Result<(), String> {
    admitted(record)?;
    if run_id.trim().is_empty() {
        return Err("native factory acceptance omitted its run identity".into());
    }
    match record.pending_admission {
        Some(Operation::Start) if record.run_id.is_none() && attempt.get() == 1 => Ok(()),
        Some(Operation::Resume)
            if record.run_id.as_deref() == Some(run_id)
                && record.attempt.and_then(|prior| prior.get().checked_add(1))
                    == Some(attempt.get()) =>
        {
            Ok(())
        }
        _ => {
            Err("factory acceptance lacks a matching start/resume dispatch and next attempt".into())
        }
    }
}

fn observed(
    record: &Record,
    run: &FactoryRunResult,
    summary: &FactoryRunSummary,
) -> Result<(), String> {
    if record.runtime_session != record.active_runtime || record.active_runtime.is_none() {
        return Err("factory observation requires an acknowledged live SDK session".into());
    }
    if run.attempt != record.attempt || run.attempt.is_none() {
        return Err("factory observation changed the correlated native attempt".into());
    }
    if summary.can_resume.is_none() {
        return Err("qualified factory detail omitted canResume".into());
    }
    Ok(())
}

fn session(record: &Record) -> Result<(), String> {
    if record.active_runtime.is_none() || record.runtime_session.is_some() {
        return Err("SDK session acceptance has no unmatched runtime opening".into());
    }
    Ok(())
}

fn rejection(record: &Record) -> Result<(), String> {
    if record.pending_admission != Some(Operation::Start) || record.run_id.is_some() {
        return Err("factory rejection lacks an unaccepted start dispatch".into());
    }
    Ok(())
}

fn closed(record: &Record, purpose: RuntimePurpose) -> Result<(), String> {
    if record.active_runtime != Some(purpose) {
        return Err("factory shutdown does not match the opened runtime purpose".into());
    }
    Ok(())
}

fn cumulative(record: &Record, summary: &FactoryRunSummary) -> Result<(), String> {
    let Some(prior) = record.consumed else {
        return Ok(());
    };
    let current = summary.consumed;
    let monotonic = current.active_ms >= prior.active_ms
        && current.subagents >= prior.subagents
        && current.nano_aiu >= prior.nano_aiu;
    if !monotonic {
        return Err(
            "factory cumulative accounting regressed below its durable high-water mark".into(),
        );
    }
    Ok(())
}

pub(super) fn snapshot(
    record: &Record,
    run: &FactoryRunResult,
    summary: &FactoryRunSummary,
) -> Result<(), String> {
    let expected = record
        .run_id
        .as_ref()
        .ok_or("factory observation has no accepted run ID")?;
    if &run.run_id != expected || summary.run_id != *expected {
        return Err("local factory observation names a different run".to_owned());
    }
    if summary.factory_name != record.intent.factory.name || summary.status != run.status {
        return Err("local factory result and detail do not describe the same snapshot".to_owned());
    }
    cumulative(record, summary)
}

pub(super) fn validate(record: &Record, data: &Data) -> Result<(), String> {
    match data {
        Data::SessionAccepted { .. } => session(record),
        Data::Dispatch { operation, .. } => dispatch(record, *operation),
        Data::Accepted {
            run_id, attempt, ..
        } => acceptance(record, run_id, *attempt),
        Data::Observed { run, summary, .. } => observed(record, run, summary),
        Data::Rejected { .. } => rejection(record),
        Data::RuntimeClosed { purpose, .. } => closed(record, *purpose),
        Data::RuntimeOpened {
            purpose: RuntimePurpose::Execution,
            ..
        } if record.active_runtime.is_some() => {
            Err("previous runtime did not shut down; execution cannot replace it".into())
        }
        _ => Ok(()),
    }
}

fn incomplete(value: &serde_json::Value) -> bool {
    let codes = [
        "interrupted",
        "factory_accounting_incomplete",
        "factory_durable_failure",
    ];
    ["type", "reason"]
        .iter()
        .filter_map(|key| value.get(key)?.as_str())
        .any(|code| codes.contains(&code))
}

pub(super) fn accounting(record: &Record) -> bool {
    let Some(run) = &record.run else { return false };
    let run_incomplete = run.reason.as_deref() == Some("interrupted")
        || run.failure.as_ref().is_some_and(incomplete);
    let terminal_incomplete = record
        .summary
        .as_ref()
        .and_then(|summary| summary.terminal.as_ref())
        .is_some_and(|terminal| {
            incomplete(terminal) || terminal.get("failure").is_some_and(incomplete)
        });
    !run_incomplete && !terminal_incomplete
}
