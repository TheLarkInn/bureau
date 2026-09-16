use bureau::github_cloud::{Dispatch, State, unsupported_control};
use serde_json::{Value, json};

use crate::cli::out;

pub(super) struct Output {
    pub(super) value: Value,
    pub(super) code: i32,
}

impl Output {
    pub(super) const fn read(value: Value) -> Self {
        Self { value, code: 0 }
    }
}

const fn submission_status(dispatch: Dispatch) -> &'static str {
    match dispatch {
        Dispatch::Prepared | Dispatch::Uncertain => "uncertain",
        dispatch => dispatch.label(),
    }
}

fn receipt_code(state: &State, operation: &str) -> i32 {
    i32::from(operation == "dispatch" && state.dispatch != Dispatch::Accepted)
}

pub(super) fn receipt(state: &State, operation: &str) -> anyhow::Result<Output> {
    let selection = state.task_id.as_ref().map(|_| "operator_selected_unproven");
    let value = json!({
        "kind": "github_cloud_receipt",
        "operation": operation,
        "submission_status": submission_status(state.dispatch),
        "task_correlation": selection,
        "remote_controls": "unsupported",
        "message": "Submission acceptance is not task completion. An explicitly selected task is not proven to originate from this submission.",
        "receipt": serde_json::to_value(state)?,
    });
    Ok(Output {
        value,
        code: receipt_code(state, operation),
    })
}

pub(super) fn events(state: &State) -> anyhow::Result<Output> {
    anyhow::ensure!(
        state.events_observed_at_ms.is_some(),
        "no recorded event snapshot; use --refresh --events"
    );
    Ok(Output::read(json!({
        "kind": "github_cloud_events",
        "task_id": state.task_id,
        "events": state.events,
        "observed_at_ms": state.events_observed_at_ms,
        "reported_total": state.events_reported_total,
        "snapshot_isolation": "not_guaranteed",
    })))
}

pub(super) fn observation(
    state: &State,
    include_events: bool,
    operation: &str,
) -> anyhow::Result<Output> {
    if include_events {
        events(state)
    } else {
        receipt(state, operation)
    }
}

fn print(output: &Output, json_output: bool) -> anyhow::Result<i32> {
    let text = if json_output {
        serde_json::to_string(&output.value)?
    } else {
        serde_json::to_string_pretty(&output.value)?
    };
    out::line(format_args!("{text}"));
    Ok(output.code)
}

pub(super) fn finish(result: anyhow::Result<Output>, json_output: bool) -> anyhow::Result<i32> {
    match result {
        Ok(output) => print(&output, json_output),
        Err(error) if json_output => print(
            &Output {
                value: json!({"kind": "github_cloud_error", "status": "error", "error": format!("{error:#}")}),
                code: 2,
            },
            true,
        ),
        Err(error) => Err(error),
    }
}

pub(in crate::cli) fn unsupported(json_output: bool) -> anyhow::Result<i32> {
    let output = Output {
        value: json!({
            "kind": "github_cloud_error", "status": "unsupported",
            "request_sent": false, "error": unsupported_control().to_string(),
        }),
        code: 2,
    };
    print(&output, json_output)
}
