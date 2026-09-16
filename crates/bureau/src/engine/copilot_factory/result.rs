//! Only a full completed result plus the original clean shutdown may finish a step.

use crate::adapters::copilot_factory::types::FactoryRunStatus;
use crate::adapters::copilot_factory::usage;
use crate::adapters::{Execution, Usage};
use crate::contract::{StepOutcome, StepResult, Trust};
use crate::process::{Secret, scrub_json};
use crate::runlog::copilot_factory::Record;

use super::super::context::RunCtx;
use super::journal::Journal;

fn unknown(message: &str) -> Execution {
    let mut execution = crate::adapters::failed(message);
    execution.result.outcome = StepOutcome::Blocked;
    execution.usage = Usage::unknown("copilot_factory");
    execution.halt()
}

pub(super) fn halt(ctx: &RunCtx, message: &str) -> Execution {
    if let Ok(journal) = Journal::new(ctx) {
        let _ = journal.pause(message);
    }
    unknown(message)
}

fn measured(record: &Record) -> Usage {
    if record.rejected.is_some() {
        return Usage::zero("copilot_factory");
    }
    if !record.accounting_complete() {
        return Usage::unknown("copilot_factory");
    }
    record
        .consumed
        .map_or_else(|| Usage::unknown("copilot_factory"), usage::measured)
}

fn terminal_result(record: &Record, secrets: &[Secret]) -> Result<StepResult, String> {
    let run = record
        .run
        .as_ref()
        .ok_or("factory has no authoritative terminal result")?;
    let mut value = run
        .result
        .clone()
        .ok_or("completed factory omitted its v2 StepResult")?;
    scrub_json(&mut value, secrets);
    let mut result = StepResult::from_json(&serde_json::to_vec(&value).map_err(|e| e.to_string())?)
        .map_err(|error| format!("factory result is not a v2 StepResult: {error}"))?;
    result.trust = Trust::Derived;
    Ok(result)
}

fn failure(record: &Record, message: &str) -> Execution {
    let mut execution = crate::adapters::failed(message);
    execution.usage = measured(record);
    execution
}

pub(super) fn execution(record: &Record, secrets: &[Secret]) -> Result<Execution, String> {
    if !record.can_clean() {
        return Err(record.diagnosis());
    }
    if let Some(message) = &record.rejected {
        return Ok(failure(record, message));
    }
    match record.status() {
        Some(FactoryRunStatus::Completed) => terminal_result(record, secrets)
            .map(|result| Execution::new(result, measured(record)))
            .or_else(|error| Ok(failure(record, &error))),
        Some(FactoryRunStatus::Cancelled) => Ok(failure(record, "native factory cancelled")),
        _ => Ok(failure(record, &record.diagnosis())),
    }
}
