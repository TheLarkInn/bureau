//! One fresh stable-v1 session and prompt, driven by the official client.

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest,
    RequestPermissionRequest, SessionNotification, SetSessionConfigOptionRequest, StopReason,
    TextContent,
};
use agent_client_protocol::{Agent, Client, ConnectTo, ConnectionTo, Result};

use super::events::{self, SharedEvents};
use super::selection;

async fn new_session(
    connection: &ConnectionTo<Agent>,
    request: NewSessionRequest,
) -> Result<agent_client_protocol::schema::v1::NewSessionResponse> {
    let response = connection
        .send_request(InitializeRequest::new(ProtocolVersion::V1))
        .block_task()
        .await?;
    if response.protocol_version != ProtocolVersion::V1 {
        return Err(selection::error(format!(
            "unsupported ACP version {}",
            response.protocol_version
        )));
    }
    connection.send_request(request).block_task().await
}

async fn select(
    connection: &ConnectionTo<Agent>,
    response: agent_client_protocol::schema::v1::NewSessionResponse,
    agent: &str,
    events: &SharedEvents,
) -> Result<agent_client_protocol::schema::v1::SessionId> {
    let options = response.config_options.as_deref().unwrap_or_default();
    selection::advertised(options, agent)?;
    events::lock(events)?.session = Some(response.session_id.clone());
    let selected = connection
        .send_request(SetSessionConfigOptionRequest::new(
            response.session_id.clone(),
            "agent",
            agent,
        ))
        .block_task()
        .await?;
    selection::selected(&selected.config_options, agent)?;
    events::lock(events)?.agent = Some(agent.to_owned());
    Ok(response.session_id)
}

fn check_cancelled(
    connection: &ConnectionTo<Agent>,
    id: &agent_client_protocol::schema::v1::SessionId,
    cancel: Option<&std::path::Path>,
) -> Result<()> {
    if cancel.is_some_and(std::path::Path::exists) {
        connection.send_notification(CancelNotification::new(id.clone()))?;
        return Err(agent_client_protocol::Error::request_cancelled());
    }
    Ok(())
}

fn completed(events: &SharedEvents, reason: StopReason) -> Result<()> {
    events::lock(events)?.check()?;
    if reason == StopReason::EndTurn {
        return Ok(());
    }
    Err(selection::error(format!(
        "ACP prompt stopped without completion: {reason:?}"
    )))
}

async fn prompt_turn(
    connection: &ConnectionTo<Agent>,
    id: agent_client_protocol::schema::v1::SessionId,
    prompt: String,
    events: &SharedEvents,
) -> Result<()> {
    let cancel = events::lock(events)?.cancel.clone();
    let request = PromptRequest::new(
        id.clone(),
        vec![ContentBlock::Text(TextContent::new(prompt))],
    );
    let response = connection.send_request(request).block_task();
    tokio::pin!(response);
    loop {
        events::lock(events)?.check()?;
        check_cancelled(connection, &id, cancel.as_deref())?;
        if let Ok(response) =
            tokio::time::timeout(std::time::Duration::from_millis(25), &mut response).await
        {
            return completed(events, response?.stop_reason);
        }
    }
}

async fn run(
    connection: ConnectionTo<Agent>,
    request: NewSessionRequest,
    agent: &str,
    prompt: String,
    events: &SharedEvents,
) -> Result<()> {
    let session = new_session(&connection, request).await?;
    let id = select(&connection, session, agent, events).await?;
    prompt_turn(&connection, id, prompt, events).await
}

pub(super) async fn exchange(
    transport: impl ConnectTo<Client> + 'static,
    request: NewSessionRequest,
    agent: String,
    prompt: String,
    events: SharedEvents,
) -> Result<()> {
    let updates = events.clone();
    let permissions = events.clone();
    Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                events::lock(&updates)?.receive(notification);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                responder.respond_with_result(events::permission(&permissions, &request))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |connection| {
            run(connection, request, &agent, prompt, &events).await
        })
        .await
}
