//! Verify the effective initialized tool graph, including deferred MCP tools.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::adapters::copilot_factory::rpc::Client;

use super::{invoke, policy, prepare::Prepared};

#[derive(Deserialize)]
struct Server {
    name: String,
    status: String,
}

#[derive(Deserialize)]
struct Servers {
    servers: Vec<Server>,
}

#[derive(Deserialize)]
struct McpTool {
    name: String,
}

#[derive(Deserialize)]
struct McpTools {
    tools: Vec<McpTool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: String,
    namespaced_name: Option<String>,
    mcp_server_name: Option<String>,
    mcp_tool_name: Option<String>,
}

#[derive(Deserialize)]
struct Metadata {
    tools: Vec<Tool>,
}

fn servers(value: Value) -> Result<bool, String> {
    let response: Servers = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let actual: Vec<_> = response
        .servers
        .into_iter()
        .map(|server| (server.name, server.status))
        .collect();
    match actual.as_slice() {
        [(name, status)] if name == "bureau-io" && status == "connected" => Ok(true),
        [(name, status)] if name == "bureau-io" && status == "pending" => Ok(false),
        _ => Err("native MCP graph is not exactly the controlled ready bureau-io server".into()),
    }
}

fn broker_tools(value: Value) -> Result<(), String> {
    let response: McpTools = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let count = response.tools.len();
    let names: BTreeSet<_> = response.tools.into_iter().map(|tool| tool.name).collect();
    if names != policy::BROKER_TOOLS.map(str::to_owned).into() || count != 2 {
        return Err("controlled bureau-io exposes a different MCP tool set".into());
    }
    Ok(())
}

fn broker(tool: &Tool) -> Result<String, String> {
    let name = tool
        .mcp_tool_name
        .as_deref()
        .ok_or("MCP metadata lacks its unqualified tool identity")?;
    if !policy::BROKER_TOOLS.contains(&name)
        || tool.namespaced_name.as_deref() != Some(&format!("bureau-io/{name}"))
        || tool.name != format!("bureau-io-{name}")
    {
        return Err(format!("unapproved native MCP tool `{}`", tool.name));
    }
    Ok(name.to_owned())
}

fn metadata(value: Value, prepared: &Prepared, execute: bool) -> Result<(), String> {
    let response: Metadata = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let allowed = policy::builtins(prepared, execute);
    let mut found = BTreeSet::new();
    for tool in response.tools {
        if tool.mcp_server_name.is_some() {
            if !execute || !found.insert(broker(&tool)?) {
                return Err("native MCP tool exceeds the exact inherited execution policy".into());
            }
        } else if tool.namespaced_name.is_some() || !allowed.contains(&tool.name) {
            return Err(format!(
                "unapproved initialized native tool `{}`",
                tool.name
            ));
        }
    }
    if execute && found != policy::BROKER_TOOLS.map(str::to_owned).into() {
        return Err("native policy omitted an approved bureau-io tool".into());
    }
    Ok(())
}

async fn connected(client: &Client, session: &str) -> Result<(), String> {
    for _ in 0..100 {
        let value = invoke::call(client, "session.mcp.list", json!({"sessionId": session})).await?;
        if servers(value)? {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err("controlled bureau-io did not finish its pending connection".into())
}

async fn mcp(client: &Client, session: &str) -> Result<(), String> {
    connected(client, session).await?;
    let value = invoke::call(
        client,
        "session.mcp.listTools",
        json!({"sessionId": session, "serverName": "bureau-io"}),
    )
    .await?;
    broker_tools(value)
}

async fn initialize(client: &Client, params: Value) -> Result<(), String> {
    let acknowledged = invoke::call(client, "session.tools.initializeAndValidate", params).await?;
    if acknowledged != json!({}) {
        return Err(
            "native tool initialization did not acknowledge the qualified empty result".into(),
        );
    }
    Ok(())
}

pub(super) async fn verify(
    client: &Client,
    prepared: &Prepared,
    execute: bool,
) -> Result<(), String> {
    let params = json!({"sessionId": prepared.intent.session_id});
    initialize(client, params.clone()).await?;
    mcp(client, &prepared.intent.session_id).await?;
    let value = invoke::call(client, "session.tools.getCurrentMetadata", params).await?;
    metadata(value, prepared, execute)
}
