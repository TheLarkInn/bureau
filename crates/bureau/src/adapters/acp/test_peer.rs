use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, Cost, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse,
    RequestPermissionRequest, SessionConfigOption, SessionConfigSelectOption, SessionNotification,
    SessionUpdate, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, StopReason,
    TextContent, ToolCallUpdate, ToolCallUpdateFields, UsageUpdate,
};
use agent_client_protocol::{Agent, ByteStreams, ConnectTo, ConnectionTo, Responder, Result};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

use super::events::{self, Events};
use super::exchange::exchange;
use crate::adapters::Usage;
use crate::process::Secret;

#[derive(Clone, Copy, Debug)]
pub(super) enum Case {
    Complete,
    StructuredResult,
    Usage(&'static str),
    MissingAgent,
    RefusedAgent,
    Drift,
    Permission,
    NoRejection,
    Error,
    Eof,
    Stalled,
    Cancelled,
}

pub(super) fn options(current: &str) -> Vec<SessionConfigOption> {
    vec![SessionConfigOption::select(
        "agent",
        "Agent",
        current.to_owned(),
        vec![
            SessionConfigSelectOption::new("default", "Default"),
            SessionConfigSelectOption::new("worker", "Display label"),
        ],
    )]
}

async fn peer(transport: impl ConnectTo<Agent> + 'static, case: Case) -> Result<()> {
    session_handlers(case)
        .on_receive_request(
            async move |r: SetSessionConfigOptionRequest, responder, _cx| {
                selected(&r, responder, case)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |r: PromptRequest, responder, cx| prompt(&r, responder, &cx, case),
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(transport)
        .await
}

fn session_handlers(
    case: Case,
) -> agent_client_protocol::Builder<
    Agent,
    impl agent_client_protocol::HandleDispatchFrom<agent_client_protocol::Client>,
> {
    Agent
        .builder()
        .on_receive_request(
            async |_r: InitializeRequest, responder, _cx| {
                responder.respond(InitializeResponse::new(ProtocolVersion::V1))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_r: NewSessionRequest, responder, _cx| {
                let choices = match case {
                    Case::MissingAgent => vec![],
                    _ => options("default"),
                };
                responder.respond(NewSessionResponse::new("session").config_options(choices))
            },
            agent_client_protocol::on_receive_request!(),
        )
}

fn selected(
    request: &SetSessionConfigOptionRequest,
    responder: Responder<SetSessionConfigOptionResponse>,
    case: Case,
) -> Result<()> {
    let value = match case {
        Case::RefusedAgent => "default",
        _ => "worker",
    };
    assert_eq!(
        (
            request.config_id.0.as_ref(),
            request.value.as_value_id().map(|v| v.0.as_ref())
        ),
        ("agent", Some("worker"))
    );
    responder.respond(SetSessionConfigOptionResponse::new(options(value)))
}

fn notify(
    connection: &ConnectionTo<agent_client_protocol::Client>,
    update: SessionUpdate,
) -> Result<()> {
    connection.send_notification(SessionNotification::new("session", update))
}

fn content(text: &str) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(TextContent::new(
        text,
    ))))
}

fn evidence(connection: &ConnectionTo<agent_client_protocol::Client>) -> Result<()> {
    notify(connection, content("prefix secret-"))?;
    notify(connection, content("token \u{20ac} suffix"))?;
    notify(
        connection,
        SessionUpdate::UsageUpdate(UsageUpdate::new(100, 200).cost(Cost::new(0.3, "USD"))),
    )?;
    notify(
        connection,
        SessionUpdate::UsageUpdate(UsageUpdate::new(120, 200).cost(Cost::new(0.4, "USD"))),
    )
}

fn structured_result(
    connection: &ConnectionTo<agent_client_protocol::Client>,
    responder: Responder<PromptResponse>,
) -> Result<()> {
    let result = crate::contract::StepResult {
        schema: crate::contract::SCHEMA_VERSION.to_owned(),
        outcome: crate::contract::StepOutcome::NoWork,
        outputs: std::collections::BTreeMap::new(),
        artifacts: Vec::new(),
        trust: crate::contract::Trust::Derived,
        message: "typed ACP result".to_owned(),
    };
    let text = String::from_utf8(result.to_json().expect("domain result")).expect("UTF-8");
    notify(connection, content(&text))?;
    responder.respond(PromptResponse::new(StopReason::EndTurn))
}

fn prompt(
    request: &PromptRequest,
    responder: Responder<PromptResponse>,
    connection: &ConnectionTo<agent_client_protocol::Client>,
    case: Case,
) -> Result<()> {
    assert_eq!(
        request.prompt,
        vec![ContentBlock::Text(TextContent::new("step request"))]
    );
    match case {
        Case::StructuredResult => structured_result(connection, responder),
        Case::Usage(report) => {
            super::usage_tests::send(connection, report)?;
            structured_result(connection, responder)
        }
        Case::Error => responder.respond_with_error(super::selection::error("peer failure")),
        Case::Stalled | Case::Eof => connection.spawn(async move {
            let _responder = responder;
            std::future::pending().await
        }),
        Case::Permission | Case::NoRejection => permission(connection, responder, case),
        Case::Drift => drift(connection, responder),
        Case::Cancelled => responder.respond(PromptResponse::new(StopReason::Cancelled)),
        _ => {
            evidence(connection)?;
            responder.respond(PromptResponse::new(StopReason::EndTurn))
        }
    }
}

fn drift(
    connection: &ConnectionTo<agent_client_protocol::Client>,
    responder: Responder<PromptResponse>,
) -> Result<()> {
    notify(
        connection,
        SessionUpdate::ConfigOptionUpdate(
            agent_client_protocol::schema::v1::ConfigOptionUpdate::new(options("default")),
        ),
    )?;
    responder.respond(PromptResponse::new(StopReason::EndTurn))
}

fn permission(
    connection: &ConnectionTo<agent_client_protocol::Client>,
    responder: Responder<PromptResponse>,
    case: Case,
) -> Result<()> {
    let mut options = vec![PermissionOption::new(
        "yes",
        "Allow",
        PermissionOptionKind::AllowOnce,
    )];
    if matches!(case, Case::Permission) {
        options.push(PermissionOption::new(
            "no",
            "Reject",
            PermissionOptionKind::RejectOnce,
        ));
    }
    let tool = ToolCallUpdate::new("call", ToolCallUpdateFields::default());
    connection
        .send_request(RequestPermissionRequest::new("session", tool, options))
        .on_receiving_result(async move |result| {
            check_permission(result, case)?;
            responder.respond(PromptResponse::new(StopReason::EndTurn))
        })
}

fn check_permission(
    response: Result<agent_client_protocol::schema::v1::RequestPermissionResponse>,
    case: Case,
) -> Result<()> {
    if matches!(case, Case::NoRejection) {
        assert!(response.is_err());
        return Ok(());
    }
    let outcome = response?.outcome;
    assert_eq!(
        serde_json::to_value(outcome).expect("outcome"),
        serde_json::json!({
            "outcome": "selected", "optionId": "no"
        })
    );
    Ok(())
}

pub(super) async fn run(case: Case) -> (Result<()>, Vec<u8>, Usage) {
    let (client, server) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(client);
    let (peer_input, peer_output) = tokio::io::split(server);
    let events = Events::new("claude", &[Secret::new("secret-token")], None, None);
    let client = exchange(
        ByteStreams::new(output.compat_write(), input.compat()),
        NewSessionRequest::new("/tmp"),
        "worker".to_owned(),
        "step request".to_owned(),
        events.clone(),
    );
    let peer = serve(
        ByteStreams::new(peer_output.compat_write(), peer_input.compat()),
        case,
    );
    let (result, _) = tokio::join!(client, peer);
    let mut events = events::lock(&events).expect("events");
    (
        result,
        events.finish().expect("capture"),
        events.usage.clone(),
    )
}

async fn serve(transport: impl ConnectTo<Agent> + 'static, case: Case) -> Result<()> {
    if matches!(case, Case::Eof) {
        let _ = tokio::time::timeout(Duration::from_millis(100), peer(transport, case)).await;
        return Ok(());
    }
    peer(transport, case).await
}
