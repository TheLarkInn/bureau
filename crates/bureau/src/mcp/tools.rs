use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::contract::{Artifact, SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust};

use super::protocol::Failure;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishArgs {
    outcome: StepOutcome,
    #[serde(default)]
    outputs: BTreeMap<String, Value>,
    #[serde(default)]
    artifacts: Vec<ArtifactArgs>,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactArgs {
    name: String,
    path: PathBuf,
}

pub(super) fn list() -> Value {
    json!({"tools": [context_definition(), publish_definition()]})
}

pub(super) fn call(
    params: Option<&Value>,
    request: &StepRequest,
    result_path: &Path,
) -> Result<Value, Failure> {
    let params = object(params, "tools/call params must be an object")?;
    let name = string(params, "name")?;
    let arguments = params.get("arguments");
    match name {
        "get_step_context" => context(arguments, request),
        "publish_result" => publish(arguments, result_path),
        _ => Err(Failure::invalid_params(format!("unknown tool {name:?}"))),
    }
}

fn context(arguments: Option<&Value>, request: &StepRequest) -> Result<Value, Failure> {
    validate_empty(arguments)?;
    let text = serde_json::to_string(request)
        .map_err(|error| Failure::invalid_params(error.to_string()))?;
    Ok(tool_result(text, false))
}

fn publish(arguments: Option<&Value>, path: &Path) -> Result<Value, Failure> {
    let value = arguments.cloned().unwrap_or_else(|| json!({}));
    let args: PublishArgs = serde_json::from_value(value)
        .map_err(|error| Failure::invalid_params(error.to_string()))?;
    let result = step_result(args);
    Ok(match publish_file(path, &result) {
        Ok(()) => tool_result("step result published", false),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            tool_result("step result was already published", true)
        }
        Err(error) => tool_result(format!("publishing step result failed: {error}"), true),
    })
}

fn step_result(args: PublishArgs) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: args.outcome,
        outputs: args.outputs,
        artifacts: args
            .artifacts
            .into_iter()
            .map(ArtifactArgs::into_artifact)
            .collect(),
        trust: Trust::Derived,
        message: args.message,
    }
}

impl ArtifactArgs {
    fn into_artifact(self) -> Artifact {
        Artifact {
            name: self.name,
            path: self.path,
        }
    }
}

fn publish_file(path: &Path, result: &StepResult) -> io::Result<()> {
    let bytes = result.to_json().map_err(io::Error::other)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()
}

fn validate_empty(arguments: Option<&Value>) -> Result<(), Failure> {
    let Some(value) = arguments else {
        return Ok(());
    };
    let object = value
        .as_object()
        .ok_or_else(|| Failure::invalid_params("tool arguments must be an object"))?;
    if object.is_empty() {
        return Ok(());
    }
    Err(Failure::invalid_params(
        "get_step_context does not accept arguments",
    ))
}

fn object<'a>(
    value: Option<&'a Value>,
    message: &'static str,
) -> Result<&'a Map<String, Value>, Failure> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| Failure::invalid_params(message))
}

fn string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, Failure> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Failure::invalid_params(format!("{field} must be a string")))
}

fn tool_result(text: impl Into<String>, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": text.into()}],
        "isError": is_error
    })
}

fn context_definition() -> Value {
    json!({
        "name": "get_step_context",
        "description": "Read the immutable request for the current step.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }
    })
}

fn publish_definition() -> Value {
    json!({
        "name": "publish_result",
        "description": "Publish the final result for the current step exactly once.",
        "inputSchema": {
            "type": "object",
            "properties": publish_properties(),
            "required": ["outcome"],
            "additionalProperties": false
        }
    })
}

fn publish_properties() -> Value {
    json!({
        "outcome": {
            "type": "string",
            "enum": ["success", "failure", "blocked", "no-work"]
        },
        "outputs": {"type": "object", "default": {}},
        "artifacts": artifact_schema(),
        "message": {"type": "string", "default": ""}
    })
}

fn artifact_schema() -> Value {
    json!({
        "type": "array",
        "default": [],
        "items": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "path": {"type": "string"}
            },
            "required": ["name", "path"],
            "additionalProperties": false
        }
    })
}
