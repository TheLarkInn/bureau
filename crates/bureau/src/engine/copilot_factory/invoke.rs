//! Raw native admission and observation, without friendly wait-wrapper timing.

use serde_json::{Value, json};

use crate::adapters::copilot_factory::rpc::{Client, Error};
use crate::adapters::copilot_factory::types::{
    FactoryRunResult, FactoryRunStatus, FactoryRunSummary,
};
use crate::config::CopilotFactoryLimits;
use crate::runlog::copilot_factory::{Data, Intent, Operation, Record};

use super::journal::Journal;

pub(super) fn error(error: Error) -> String {
    match error {
        Error::Remote(fault) => format!(
            "{}: {} (JSON-RPC {})",
            fault.data_code().unwrap_or("unclassified_runtime_error"),
            fault.message,
            fault.code
        ),
        other => other.to_string(),
    }
}

pub(super) async fn call(client: &Client, method: &str, params: Value) -> Result<Value, String> {
    client.call(method, params).await.map_err(error)
}

fn limits(limits: &CopilotFactoryLimits) -> Value {
    let values = [
        (
            "maxConcurrentSubagents",
            limits.max_concurrent_subagents.map(Value::from),
        ),
        (
            "maxTotalSubagents",
            limits.max_total_subagents.map(Value::from),
        ),
        ("timeoutSeconds", limits.timeout_seconds.map(Value::from)),
        ("maxAiCredits", limits.max_ai_credits.map(Value::from)),
    ];
    Value::Object(
        values
            .into_iter()
            .filter_map(|(key, value)| value.map(|v| (key.to_owned(), v)))
            .collect(),
    )
}

pub(super) async fn start(client: &Client, intent: &Intent) -> Result<(), String> {
    let mut params = json!({"sessionId": intent.session_id,
        "name": intent.factory.name, "args": intent.factory.args});
    if let Some(overrides) = &intent.factory.limits {
        params["options"] = json!({"limits": limits(overrides)});
    }
    call(client, "session.factory.run", params)
        .await
        .map(|_| ())
}

pub(super) async fn resume(client: &Client, record: &Record) -> Result<(), String> {
    if !record.can_resume() {
        return Err(
            "runtime run is not explicitly resumable; no replacement start was sent".into(),
        );
    }
    call(
        client,
        "session.factory.resume",
        json!({
            "sessionId": record.intent.session_id, "runId": record.run_id,
        }),
    )
    .await
    .map(|_| ())
}

pub(super) fn parameters(record: &Record) -> Result<Value, String> {
    let run_id = record
        .run_id
        .as_ref()
        .ok_or("factory has no correlated accepted run ID")?;
    Ok(json!({"sessionId": record.intent.session_id, "runId": run_id}))
}

async fn snapshot(
    client: &Client,
    params: Value,
) -> Result<(FactoryRunResult, FactoryRunSummary), String> {
    let value = call(client, "session.factory.getRun", params.clone()).await?;
    let run: FactoryRunResult = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let detail = call(client, "session.factory.getRunDetail", params).await?;
    // SDK FactoryRunDetail flattens FactoryRunSummary; there is no `run` container.
    let summary: FactoryRunSummary = serde_json::from_value(detail).map_err(|e| e.to_string())?;
    Ok((run, summary))
}

const fn advancing(
    run: FactoryRunStatus,
    detail: FactoryRunStatus,
    operation: Option<Operation>,
) -> bool {
    match run {
        FactoryRunStatus::Pending => !matches!(detail, FactoryRunStatus::Pending),
        FactoryRunStatus::Running => detail.is_settled(),
        FactoryRunStatus::Paused | FactoryRunStatus::Halted | FactoryRunStatus::Error => {
            matches!(
                (detail, operation),
                (FactoryRunStatus::Cancelled, Some(Operation::Cancel))
            )
        }
        _ => false,
    }
}

async fn consistent(
    client: &Client,
    params: Value,
    journal: &Journal,
    session: &str,
) -> Result<(FactoryRunResult, FactoryRunSummary), String> {
    for _ in 0..3 {
        let (run, summary) = snapshot(client, params.clone()).await?;
        if run.run_id != summary.run_id {
            return Err("factory result and detail changed run identity".into());
        }
        if run.status == summary.status {
            return Ok((run, summary));
        }
        if !advancing(
            run.status,
            summary.status,
            journal.record(session)?.dispatched,
        ) {
            return Err(
                "factory result/detail states regressed or changed terminal outcome".into(),
            );
        }
    }
    Err("factory result/detail did not reach a consistent authoritative observation".into())
}

pub(super) async fn observe(
    client: &Client,
    journal: &Journal,
    session: &str,
) -> Result<Record, String> {
    let params = parameters(&journal.record(session)?)?;
    let (run, summary) = consistent(client, params, journal, session).await?;
    journal
        .append(Data::Observed {
            session_id: session.to_owned(),
            run: Box::new(run),
            summary: Box::new(summary),
        })
        .map_err(|error| error.to_string())?;
    journal.record(session)
}
