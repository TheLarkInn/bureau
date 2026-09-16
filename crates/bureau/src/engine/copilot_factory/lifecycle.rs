//! Poll storage-backed native state; notifications are only low-latency hints.

use std::path::Path;
use std::time::Duration;

use crate::adapters::copilot_factory::rpc::Client;
use crate::adapters::copilot_factory::types::FactoryRunStatus;
use crate::runlog::copilot_factory::{Intent, Operation, Record};

use super::{
    invoke,
    journal::Journal,
    race::{self, First},
};

pub(super) fn control(directory: &Path) -> Option<Operation> {
    if directory.join("CANCEL").exists() {
        return Some(Operation::Cancel);
    }
    directory.join("PAUSE").exists().then_some(Operation::Pause)
}

async fn stop(client: &Client, record: &Record, operation: Operation) -> Result<(), String> {
    let method = match operation {
        Operation::Cancel => "session.factory.cancel",
        Operation::Pause => "session.factory.pause",
        Operation::Start | Operation::Resume => return Err("invalid factory stop operation".into()),
    };
    invoke::call(client, method, invoke::parameters(record)?)
        .await
        .map(|_| ())
}

async fn controls(
    client: &Client,
    record: &Record,
    directory: &Path,
    sent: &mut Option<Operation>,
) -> Result<(), String> {
    let requested = control(directory);
    if let Some(operation) = requested.filter(|operation| Some(*operation) != *sent) {
        stop(client, record, operation).await?;
        *sent = Some(operation);
    }
    Ok(())
}

async fn watch_controls(
    client: &Client,
    journal: &Journal,
    session: &str,
    directory: &Path,
    mut sent: Option<Operation>,
) -> Result<(), String> {
    loop {
        let record = journal.record(session)?;
        controls(client, &record, directory, &mut sent).await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn settled_state(record: &Record, directory: &Path) -> bool {
    if matches!(
        record.status(),
        Some(FactoryRunStatus::Paused | FactoryRunStatus::Halted)
    ) && control(directory) == Some(Operation::Cancel)
    {
        return false;
    }
    record.status().is_some_and(FactoryRunStatus::is_settled)
}

fn preserve_stop(journal: &Journal, record: &Record) -> Result<(), String> {
    let resumable = record
        .summary
        .as_ref()
        .is_some_and(|summary| summary.can_resume == Some(true));
    let stopped = matches!(
        record.status(),
        Some(FactoryRunStatus::Paused | FactoryRunStatus::Halted)
    );
    if stopped || resumable || !record.accounting_complete() {
        journal
            .pause("native factory stopped; preserved for explicit inspection or resume")
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn observed_control(client: &Client, journal: &Journal, session: &str) -> Result<(), String> {
    let record = invoke::observe(client, journal, session).await?;
    preserve_stop(journal, &record)
}

async fn observations(
    client: &Client,
    journal: &Journal,
    session: &str,
    directory: &Path,
) -> Result<(), String> {
    loop {
        let record = invoke::observe(client, journal, session).await?;
        if settled_state(&record, directory) {
            preserve_stop(journal, &record)?;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn settled(
    client: &Client,
    journal: &Journal,
    session: &str,
    directory: &Path,
    sent: Option<Operation>,
) -> Result<(), String> {
    match race::first(
        observations(client, journal, session, directory),
        watch_controls(client, journal, session, directory, sent),
    )
    .await
    {
        First::Left(result) | First::Right(result) => result,
    }
}

async fn continued(client: &Client, record: &Record, operation: Operation) -> Result<(), String> {
    match operation {
        Operation::Start => Err("start is not a continuation operation".into()),
        Operation::Resume => invoke::resume(client, record).await,
        Operation::Pause | Operation::Cancel => stop(client, record, operation).await,
    }
}

async fn admit(
    client: &Client,
    record: &Record,
    operation: Operation,
    intent: &Intent,
) -> Result<(), String> {
    if operation == Operation::Start {
        invoke::start(client, intent).await
    } else {
        continued(client, record, operation).await
    }
}

fn admission_directory(record: &Record, operation: Operation) -> Result<&Path, String> {
    let directory = record
        .intent
        .workspace
        .directory
        .parent()
        .ok_or("worktree has no run directory")?;
    if super::session::executes(Some(operation)) && control(directory).is_some() {
        return Err(
            "factory admission was stopped by a run control marker during initialization".into(),
        );
    }
    Ok(directory)
}

async fn active(
    client: &Client,
    journal: &Journal,
    record: &Record,
    operation: Operation,
    intent: &Intent,
) -> Result<(), String> {
    let directory = admission_directory(record, operation)?;
    admit(client, record, operation, intent).await?;
    // Inspection cannot settle an executor owned by another runtime.
    if !super::session::executes(Some(operation)) {
        return observed_control(client, journal, &record.intent.session_id).await;
    }
    settled(client, journal, &record.intent.session_id, directory, None).await
}

pub(super) async fn run(
    client: &Client,
    journal: &Journal,
    record: &Record,
    operation: Option<Operation>,
    intent: &Intent,
) -> Result<(), String> {
    match operation {
        Some(operation) => active(client, journal, record, operation, intent).await,
        None => invoke::observe(client, journal, &record.intent.session_id)
            .await
            .map(|_| ()),
    }
}
