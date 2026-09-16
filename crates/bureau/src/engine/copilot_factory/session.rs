//! Qualified SDK handshake and the create-only activation boundary.

use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};

use crate::adapters::copilot_factory::rpc::Client;
use crate::mcp::Session;
use crate::runlog::copilot_factory::{Operation, Record};

use super::{catalog, invoke, policy, prepare::Prepared};

pub(super) const fn executes(operation: Option<Operation>) -> bool {
    matches!(operation, Some(Operation::Start | Operation::Resume))
}

async fn handshake(client: &Client, prepared: &Prepared) -> Result<(), String> {
    let connected = invoke::call(client, "connect", json!({})).await?;
    if connected["ok"] != true
        || connected["protocolVersion"] != 3
        || connected["version"].as_str() != Some(&prepared.intent.factory.runtime.version)
    {
        return Err(
            "runtime handshake does not match the approved SDK artifact, protocol, and version"
                .into(),
        );
    }
    let registered = invoke::call(client, "registerExtensionLaunchProvider", json!({})).await?;
    if !registered.is_null() {
        return Err("extension launch-provider registration did not acknowledge null".into());
    }
    Ok(())
}

fn parameters(
    prepared: &Prepared,
    broker: &Session,
    record: &Record,
    execute: bool,
) -> Result<Value, String> {
    let mut params = policy::parameters(prepared, broker, execute)?;
    params["sessionId"] = json!(prepared.intent.session_id);
    params["gitHubToken"] = json!(prepared.model_token.expose());
    params["requestExtensions"] = json!(execute);
    params["requestPermission"] = json!(true);
    if !record.session_accepted {
        params["workingDirectory"] = json!(prepared.intent.workspace.directory);
        params["trustWorkingDirectory"] = json!("session");
        params["enableConfigDiscovery"] = json!(false);
        params["pluginDirectories"] = json!(prepared.intent.context.plugin_directories());
    }
    if let Some(agent) = &prepared.intent.context.custom_agent {
        params["customAgents"] = json!([agent]);
    }
    Ok(params)
}

async fn initialize(
    client: &Client,
    prepared: &Prepared,
    broker: &Session,
    record: &Record,
    execute: bool,
) -> Result<(), String> {
    let method = if record.session_accepted {
        "session.resume"
    } else {
        "session.create"
    };
    invoke::call(
        client,
        method,
        parameters(prepared, broker, record, execute)?,
    )
    .await?;
    Ok(())
}

pub(super) async fn open(
    client: &Client,
    prepared: &Prepared,
    broker: &Session,
    record: &Record,
    operation: Option<Operation>,
    launched: &AtomicBool,
) -> Result<(), String> {
    handshake(client, prepared).await?;
    initialize(client, prepared, broker, record, executes(operation)).await?;
    catalog::verify(client, prepared, executes(operation), launched).await
}
