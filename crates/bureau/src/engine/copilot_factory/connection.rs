//! One bounded SDK connection, with checked shutdown and generation-loss cleanup.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use tokio::io::BufReader;
use tokio::process::{ChildStdin, ChildStdout};
use tokio::time::Instant;

use crate::adapters::copilot_factory::rpc::Client;
use crate::config::StepDef;
use crate::mcp::Session;
use crate::process::SpawnResult;
use crate::runlog::copilot_factory::{Data, Operation, Record, RuntimePurpose};

use super::super::context::RunCtx;
use super::journal::Journal;
use super::prepare::Prepared;
use super::{
    authentication, launch, lifecycle,
    observer::Observer,
    race::{self, First},
    session, supervised,
};

struct Pass<'a> {
    prepared: &'a Prepared,
    journal: &'a Journal,
    broker: &'a Session,
    record: Record,
    operation: Option<Operation>,
}

struct Reply {
    outcome: Result<(), String>,
    acknowledged: bool,
}

async fn shutdown(client: &Client) -> bool {
    client
        .call_without_params("runtime.shutdown")
        .await
        .is_ok_and(|value| value.is_null())
}

fn observer(pass: &Pass<'_>, launched: Arc<AtomicBool>) -> Observer {
    Observer {
        journal: pass.journal.clone(),
        session: pass.prepared.intent.session_id.clone(),
        artifacts: pass.prepared.artifacts.clone(),
        request_file: pass.broker.request_path().to_path_buf(),
        agent: pass.prepared.intent.context.selected_agent.clone(),
        launched,
        allow_provider: session::executes(pass.operation),
    }
}

async fn operate(client: &Client, pass: &Pass<'_>, launched: &AtomicBool) -> Result<(), String> {
    session::open(
        client,
        pass.prepared,
        pass.broker,
        &pass.record,
        pass.operation,
        launched,
    )
    .await?;
    lifecycle::run(
        client,
        pass.journal,
        &pass.record,
        pass.operation,
        &pass.prepared.intent,
    )
    .await
}

fn interrupted(pass: &Pass<'_>, outcome: &Result<(), String>) -> Result<(), String> {
    let Err(message) = outcome else { return Ok(()) };
    let record = pass.journal.record(&pass.prepared.intent.session_id)?;
    if record.dispatched.is_some() && record.rejected.is_none() {
        pass.journal
            .append(Data::Indeterminate {
                session_id: pass.prepared.intent.session_id.clone(),
                message: message.clone(),
            })
            .map_err(|error| error.to_string())?;
    }
    pass.journal
        .pause(message)
        .map_err(|error| error.to_string())
}

async fn interact(stdin: ChildStdin, stdout: ChildStdout, pass: &Pass<'_>) -> Reply {
    let launched = Arc::new(AtomicBool::new(false));
    let client = Client::start(
        BufReader::new(stdout),
        stdin,
        observer(pass, launched.clone()),
    );
    let outcome = operate(&client, pass, &launched).await;
    let outcome = interrupted(pass, &outcome).and(outcome);
    let acknowledged = shutdown(&client).await;
    Reply {
        outcome,
        acknowledged,
    }
}

const fn purpose(operation: Option<Operation>) -> RuntimePurpose {
    if session::executes(operation) {
        RuntimePurpose::Execution
    } else {
        RuntimePurpose::Inspection
    }
}

fn closed(pass: &Pass<'_>, process: &SpawnResult, reply: Option<Reply>) -> Result<(), String> {
    let clean =
        supervised::clean(process) && reply.as_ref().is_some_and(|reply| reply.acknowledged);
    let message = format!(
        "factory runtime shutdown: {:?}, exit {:?}, acknowledged {clean}",
        process.outcome, process.exit_code
    );
    pass.journal
        .append(Data::RuntimeClosed {
            session_id: pass.prepared.intent.session_id.clone(),
            purpose: purpose(pass.operation),
            clean,
            message: message.clone(),
        })
        .map_err(|error| error.to_string())?;
    if !clean {
        return Err(message);
    }
    reply.ok_or("factory interaction was interrupted")?.outcome
}

async fn owned(journal: &Journal) -> String {
    loop {
        if let Err(error) = journal.check() {
            return error.to_string();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn supervised_pass(
    request: crate::process::SpawnRequest,
    pass: &Pass<'_>,
) -> Result<(), String> {
    let process = supervised::run(request, |stdin, stdout| interact(stdin, stdout, pass));
    match race::first(process, owned(pass.journal)).await {
        First::Left((process, reply)) => closed(pass, &process, reply),
        First::Right(error) => Err(error),
    }
}

fn timeout(ctx: &RunCtx, deadline: Instant, operation: Option<Operation>) -> Duration {
    let remaining = super::super::deadline::remaining(deadline);
    if session::executes(operation) {
        remaining.min(ctx.remaining())
    } else {
        remaining
    }
}

fn verify(prepared: &Prepared) -> Result<(), String> {
    prepared.artifacts.verify()?;
    authentication::verify(
        &prepared.artifacts.paths.storage.copilot_home,
        &prepared.intent.context,
    )?;
    super::context::restore(&prepared.intent.context)
}

pub(super) async fn pass(
    ctx: &RunCtx,
    step: &StepDef,
    prepared: &Prepared,
    deadline: Instant,
    journal: &Journal,
    operation: Option<Operation>,
) -> Result<(), String> {
    verify(prepared)?;
    let record = journal.record(&prepared.intent.session_id)?;
    let broker = launch::broker(prepared)?;
    let request = launch::request(ctx, step, prepared, timeout(ctx, deadline, operation));
    journal
        .append(Data::RuntimeOpened {
            session_id: prepared.intent.session_id.clone(),
            purpose: purpose(operation),
        })
        .map_err(|error| error.to_string())?;
    let pass = Pass {
        prepared,
        journal,
        broker: &broker,
        record,
        operation,
    };
    supervised_pass(request, &pass).await?;
    verify(prepared)
}
