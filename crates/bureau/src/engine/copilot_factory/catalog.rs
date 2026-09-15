//! Pinned-provider readiness over Bureau's required SDK capability contract.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{invoke, prepare::Prepared, resources, tools};
use crate::adapters::copilot_factory::rpc::Client;

#[derive(Deserialize)]
struct Extension {
    id: String,
    name: String,
    source: String,
    status: String,
}

#[derive(Deserialize)]
struct ExtensionList {
    extensions: Vec<Extension>,
}

fn state(extension: &Extension, prepared: &Prepared) -> Result<bool, String> {
    if extension.name != prepared.artifacts.identity.runtime_extension_name
        || extension.source != "session"
    {
        return Err("factory extension catalog changed the approved provider identity".into());
    }
    match extension.status.as_str() {
        "running" => Ok(true),
        "starting" => Ok(false),
        other => Err(format!(
            "approved factory extension is not running: {other}"
        )),
    }
}

fn ready(value: Value, prepared: &Prepared) -> Result<bool, String> {
    let response: ExtensionList =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    let expected = &prepared.artifacts.identity.runtime_extension_id;
    let mut matches = response
        .extensions
        .iter()
        .filter(|extension| &extension.id == expected);
    let Some(extension) = matches.next() else {
        return Ok(false);
    };
    if matches.next().is_some() {
        return Err("extension catalog repeats the approved provider identity".into());
    }
    state(extension, prepared)
}

async fn provider(
    client: &Client,
    prepared: &Prepared,
    launched: &AtomicBool,
) -> Result<(), String> {
    let params = json!({"sessionId": prepared.intent.session_id});
    for _ in 0..100 {
        let value = invoke::call(client, "session.extensions.list", params.clone()).await?;
        if ready(value, prepared)? && launched.load(Ordering::Acquire) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("approved factory provider did not become ready after its checked launch".into())
}

async fn context(client: &Client, prepared: &Prepared, execute: bool) -> Result<(), String> {
    tools::verify(client, prepared, execute).await?;
    resources::verify(
        client,
        &prepared.intent.session_id,
        &prepared.intent.context,
    )
    .await
}

async fn eligibility(client: &Client, prepared: &Prepared, execute: bool) -> Result<(), String> {
    context(client, prepared, execute).await?;
    let value = invoke::call(
        client,
        "session.factory.listRuns",
        json!({"sessionId": prepared.intent.session_id}),
    )
    .await?;
    if !value.get("runs").is_some_and(Value::is_array) {
        return Err("factory eligibility response did not contain the qualified runs array".into());
    }
    Ok(())
}

pub(super) async fn verify(
    client: &Client,
    prepared: &Prepared,
    execute: bool,
    launched: &AtomicBool,
) -> Result<(), String> {
    if execute {
        provider(client, prepared, launched).await?;
    }
    eligibility(client, prepared, execute).await
}
