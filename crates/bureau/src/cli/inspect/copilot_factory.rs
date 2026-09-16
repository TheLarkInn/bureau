//! Local-runtime evidence, never GitHub cloud receipt/task state.

use bureau::adapters::copilot_factory::types::FactoryRunStatus;
use bureau::runlog::RunState;
use bureau::runlog::copilot_factory::Record;
use std::path::Path;

use crate::cli::out;
use anyhow::Context as _;

pub(super) fn current(state: &RunState) -> Option<&Record> {
    let step = state.steps.last().filter(|step| step.outcome.is_none())?;
    state
        .copilot_factories
        .0
        .values()
        .filter(|record| record.intent.step == step.step)
        .max_by_key(|record| record.intent.step_attempt)
}

fn status(record: &Record) -> String {
    if record.indeterminate.is_some() || record.ambiguous_start() {
        return "indeterminate".into();
    }
    record
        .status()
        .map_or_else(|| "not admitted".into(), |status| format!("{status:?}"))
}

pub(super) fn list_suffix(state: &RunState) -> String {
    if state.copilot_factory_error.is_some() {
        return "  local-factory:invalid-log".into();
    }
    current(state).map_or_else(String::new, |record| {
        format!(
            "  local-factory:{} {}",
            status(record),
            record.run_id.as_deref().unwrap_or("no accepted run ID")
        )
    })
}

fn print_usage(record: &Record) {
    match record.consumed {
        Some(usage) => out::line(format_args!(
            "    usage: {} nano-AIU; {} subagents; {} ms active; accounting complete: {}",
            usage.nano_aiu,
            usage.subagents,
            usage.active_ms,
            record.accounting_complete()
        )),
        None => out::line(format_args!("    usage: unavailable, not zero")),
    }
}

fn print_identity(record: &Record) {
    let intent = &record.intent;
    out::line(format_args!(
        "  {}: local factory {}",
        intent.step, intent.factory.name
    ));
    out::line(format_args!("    SDK session: {}", intent.session_id));
    out::line(format_args!(
        "    native run: {}",
        record.run_id.as_deref().unwrap_or("not accepted")
    ));
    out::line(format_args!(
        "    native status: {}; attempt: {:?}",
        status(record),
        record.attempt
    ));
}

fn print_material(record: &Record) {
    let intent = &record.intent;
    out::line(format_args!(
        "    model credential reference: {}",
        intent.factory.model_credential
    ));
    out::line(format_args!(
        "    provider: {} -> {}",
        intent.identity.source_extension_id, intent.identity.runtime_extension_id
    ));
    out::line(format_args!(
        "    runtime: {}; digest: {}",
        intent.factory.runtime.version, intent.identity.runtime_digest
    ));
    out::line(format_args!(
        "    workspace: {}",
        intent.workspace.directory.display()
    ));
    out::line(format_args!(
        "    runtime storage: {}",
        intent.paths.storage.copilot_home.display()
    ));
}

fn print_record(record: &Record) {
    print_identity(record);
    print_material(record);
    print_usage(record);
    out::line(format_args!("    recovery: {}", record.diagnosis()));
}

pub(super) fn print_state(state: &RunState) {
    for record in state.copilot_factories.0.values() {
        print_record(record);
    }
    if let Some(error) = &state.copilot_factory_error {
        out::line(format_args!("local factory log invalid: {error}"));
    }
}

fn record_resumable(record: &Record) -> bool {
    if record.can_start() {
        return true;
    }
    if record.indeterminate.is_some() || !record.execution_clean || !record.accounting_complete() {
        return false;
    }
    record.can_resume() || record.status() == Some(FactoryRunStatus::Completed)
}

pub(super) fn check_state(state: &RunState) -> anyhow::Result<()> {
    if let Some(error) = &state.copilot_factory_error {
        anyhow::bail!("local factory log invalid; preserve runtime and workspace: {error}");
    }
    if let Some(record) = current(state) {
        anyhow::ensure!(record_resumable(record), "{}", record.diagnosis());
    }
    Ok(())
}

pub(super) fn check_resume(directory: &Path) -> anyhow::Result<()> {
    let events = bureau::runlog::read_events_tolerant(directory)
        .context("checking local factory resume evidence")?;
    if !events
        .iter()
        .any(|event| event.kind == bureau::runlog::EventKind::CopilotFactory)
    {
        return Ok(());
    }
    let state =
        bureau::runlog::replay(events).context("factory run log has no run_started event")?;
    check_state(&state)
}
