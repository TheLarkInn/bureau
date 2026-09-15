//! Inspect the preserved identity before deciding whether execution may resume.

use std::time::Duration;

use crate::config::StepDef;
use crate::runlog::copilot_factory::{Data, Operation, Record};

use super::super::context::RunCtx;
use super::journal::Journal;
use super::prepare::Prepared;
use super::{connection, lifecycle, recovery};

fn uncertain(prepared: &Prepared, journal: &Journal, message: String) -> Result<(), String> {
    journal
        .append(Data::Indeterminate {
            session_id: prepared.intent.session_id.clone(),
            message: message.clone(),
        })
        .map_err(|error| error.to_string())?;
    Err(message)
}

fn recovery_operation(record: &Record) -> Result<Option<Operation>, String> {
    if record.can_clean() {
        return Ok(None);
    }
    let directory = record
        .intent
        .workspace
        .directory
        .parent()
        .ok_or("factory run directory is absent")?;
    if lifecycle::control(directory) == Some(Operation::Cancel) {
        return Ok(Some(Operation::Cancel));
    }
    if record.can_resume() {
        return Ok(Some(Operation::Resume));
    }
    Err(record.diagnosis())
}

fn start_needed(record: &Record) -> Result<bool, String> {
    if record.run_id.is_none() && record.indeterminate.is_some() {
        return Err(record.diagnosis());
    }
    if !record.execution_opened {
        return Ok(true);
    }
    if record.run_id.is_none() && !record.can_start() {
        return Err(record.diagnosis());
    }
    recovery::runtime_state(record)?;
    Ok(record.run_id.is_none())
}

fn admission_control(ctx: &RunCtx, prepared: &Prepared) -> Result<(), String> {
    if prepared
        .recovered
        .as_ref()
        .is_some_and(|record| record.run_id.is_some())
    {
        return Ok(());
    }
    let directory = super::super::context::run_dir(ctx);
    if let Some(operation) = lifecycle::control(&directory) {
        return Err(format!(
            "factory admission was not dispatched because {operation:?} was requested"
        ));
    }
    Ok(())
}

fn failed(prepared: &Prepared, journal: &Journal, error: String) -> Result<(), String> {
    let record = journal.record(&prepared.intent.session_id)?;
    if record.rejected.is_some() && record.execution_clean {
        return Ok(());
    }
    if record.can_start() {
        return Err(error);
    }
    uncertain(prepared, journal, error)
}

fn control_timeout(ctx: &RunCtx, timeout: Duration) -> Duration {
    if timeout.is_zero() && ctx.cancel_path().exists() {
        Duration::from_secs(5)
    } else {
        timeout
    }
}

struct Invocation<'a> {
    ctx: &'a RunCtx,
    step: &'a StepDef,
    prepared: &'a Prepared,
    journal: &'a Journal,
    deadline: tokio::time::Instant,
}

impl Invocation<'_> {
    async fn pass(&self, operation: Option<Operation>) -> Result<(), String> {
        connection::pass(
            self.ctx,
            self.step,
            self.prepared,
            self.deadline,
            self.journal,
            operation,
        )
        .await
    }

    async fn inspect(&self) -> Result<(), String> {
        self.pass(None).await?;
        let record = self.journal.record(&self.prepared.intent.session_id)?;
        if let Some(operation) = recovery_operation(&record)? {
            self.pass(Some(operation)).await?;
        }
        Ok(())
    }

    async fn recover(&self) -> Result<(), String> {
        let initial = self.journal.record(&self.prepared.intent.session_id)?;
        if start_needed(&initial)? {
            return self.pass(Some(Operation::Start)).await;
        }
        self.inspect().await
    }

    async fn execute(&self) -> Result<(), String> {
        admission_control(self.ctx, self.prepared)?;
        let outcome = if self.prepared.recovered.is_some() {
            self.recover().await
        } else {
            self.pass(Some(Operation::Start)).await
        };
        outcome.or_else(|error| failed(self.prepared, self.journal, error))
    }
}

pub(super) async fn execute(
    ctx: &RunCtx,
    step: &StepDef,
    prepared: &Prepared,
    timeout: Duration,
    journal: &Journal,
) -> Result<(), String> {
    let invocation = Invocation {
        ctx,
        step,
        prepared,
        journal,
        deadline: super::super::deadline::after(control_timeout(ctx, timeout)),
    };
    invocation.execute().await
}
