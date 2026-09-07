//! Shared official ACP v1 transport. Bureau retains process and domain ownership.

mod capture;
mod events;
mod exchange;
mod selection;
mod setup;

#[cfg(test)]
mod result_tests;
#[cfg(test)]
mod test_peer;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wire_tests;

use agent_client_protocol::schema::v1::NewSessionRequest;
use agent_client_protocol::{ByteStreams, Error};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

use crate::config::{AdapterKind, Role};
use crate::contract::{StepRequest, StepResult};
use crate::mcp::Session;
use crate::process::{Secret, SpawnRequest, SpawnResult};

use super::{Execution, Usage};
use events::{Events, SharedEvents};

struct Prepared {
    session: NewSessionRequest,
    agent: String,
    prompt: String,
}

fn prepare(role: &Role, request: &StepRequest, session: &Session) -> Result<Prepared, String> {
    let agent = super::resolved_agent(role, &request.worktree);
    let prompt = String::from_utf8(request.to_json().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    Ok(Prepared {
        session: setup::new_session(role, request, session, &agent)?,
        agent,
        prompt,
    })
}

const fn provider(role: &Role) -> &'static str {
    match role.adapter {
        AdapterKind::Copilot => "copilot",
        AdapterKind::Claude => "claude",
        AdapterKind::Fake => "fake",
    }
}

async fn run(
    built: SpawnRequest,
    prepared: Prepared,
    events: SharedEvents,
) -> (SpawnResult, Option<Result<(), Error>>) {
    crate::process::duplex(built, |stdin, stdout| {
        exchange::exchange(
            ByteStreams::new(stdin.compat_write(), stdout.compat()),
            prepared.session,
            prepared.agent,
            prepared.prompt,
            events,
        )
    })
    .await
}

fn protocol_error(error: &Error, secrets: &[Secret]) -> String {
    let mut value = match serde_json::to_value(error) {
        Ok(value) => value,
        Err(error) => return format!("encoding ACP error failed: {error}"),
    };
    crate::process::scrub_json(&mut value, secrets);
    format!("ACP exchange failed: {value}")
}

fn completed(
    spawned: &SpawnResult,
    exchange: Option<Result<(), Error>>,
    secrets: &[Secret],
) -> Result<(), String> {
    if !super::successful(spawned) || spawned.error.is_some() {
        return Err(format!(
            "ACP process ended {:?}: {}",
            spawned.outcome,
            super::tail(spawned)
        ));
    }
    match exchange {
        Some(Ok(())) => Ok(()),
        Some(Err(error)) => Err(protocol_error(&error, secrets)),
        None => Err("ACP connection ended before the prompt completed".to_owned()),
    }
}

fn domain_result(session: &Session, response: &[u8]) -> Result<StepResult, String> {
    let published = session
        .published()
        .map_err(|error| format!("reading published result failed: {error}"))?;
    Ok(published.unwrap_or_else(|| {
        super::transcript::result_from_output(response).unwrap_or_else(|_| super::missing_result())
    }))
}

fn failure(message: &str, secrets: &[Secret]) -> StepResult {
    capture::scrub_error(message, secrets).map_or_else(
        |_| super::failed("scrubbing ACP failure detail failed").result,
        |message| super::failed(&message).result,
    )
}

fn finish(
    session: &Session,
    events: &SharedEvents,
    completion: Result<(), String>,
    secrets: &[Secret],
) -> Execution {
    let mut state = match events::lock(events) {
        Ok(state) => state,
        Err(error) => return super::failed(&protocol_error(&error, secrets)),
    };
    let completion = completion.and_then(|()| {
        state
            .check()
            .map_err(|error| protocol_error(&error, secrets))
    });
    let response = state
        .finish()
        .map_err(|error| protocol_error(&error, secrets));
    let result = completion
        .and(response)
        .and_then(|response| domain_result(session, &response));
    Execution::new(
        result.unwrap_or_else(|message| failure(&message, secrets)),
        state.usage.clone(),
    )
}

pub(super) async fn execute(
    role: &Role,
    request: &StepRequest,
    session: &Session,
    built: SpawnRequest,
) -> Execution {
    let prepared = match prepare(role, request, session) {
        Ok(prepared) => prepared,
        Err(message) => {
            return Execution::new(
                failure(&message, &built.secrets),
                Usage::unknown(provider(role)),
            );
        }
    };
    let secrets = built.secrets.clone();
    let events = Events::new(
        provider(role),
        &secrets,
        built.log.clone(),
        built.cancel.clone(),
    );
    let (spawned, exchanged) = run(built, prepared, events.clone()).await;
    finish(
        session,
        &events,
        completed(&spawned, exchanged, &secrets),
        &secrets,
    )
}
