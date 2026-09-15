//! The same approved metadata imported by the extension's `defineFactory` call.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{SetupError, schema};

fn phase_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["title"],
        "properties": {"title": {"type": "string"}, "detail": {"type": "string"}}
    })
}

fn limits_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "maxConcurrentSubagents": {"type": "integer", "minimum": 1, "maximum": 500},
            "maxTotalSubagents": {"type": "integer", "minimum": 1},
            "timeoutSeconds": {"type": "number", "exclusiveMinimum": 0},
            "maxAiCredits": {"type": "number", "exclusiveMinimum": 0}
        }
    })
}

fn metadata_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["name", "description", "phases"],
        "properties": {
            "name": {"type": "string", "minLength": 1, "pattern": "^[A-Za-z0-9_-]+$"},
            "description": {"type": "string"},
            "phases": {"type": "array", "items": phase_schema()},
            "argsSchema": {"type": ["object", "boolean"]},
            "limits": limits_schema()
        }
    })
}

/// One phase label from the SDK factory metadata.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    /// Human-readable phase title.
    pub title: String,
    /// Optional detail shown with the phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Pinned SDK `FactoryMeta`, not text scraped from a catalog or result preview.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Definition {
    /// Registered factory name.
    pub name: String,
    /// Description supplied to the SDK.
    pub description: String,
    /// Declared phases.
    pub phases: Vec<Phase>,
    /// Complete declared JSON Schema, when provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_schema: Option<Value>,
    /// Validated native metadata limits, retaining exact JSON number values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Value>,
}

impl Definition {
    fn check_name(&self, expected: &str) -> Result<(), String> {
        if self.name == expected {
            Ok(())
        } else {
            Err("pinned factory metadata name does not match copilot_factory.name".to_owned())
        }
    }

    fn check_arguments(&self, arguments: &Value) -> Result<(), String> {
        self.args_schema.as_ref().map_or(Ok(()), |schema| {
            schema::validate(schema, arguments).map_err(String::from)
        })
    }

    fn parse_inner(bytes: &[u8], expected: &str, arguments: &Value) -> Result<Self, String> {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid factory metadata JSON: {error}"))?;
        schema::validate(&metadata_schema(), &value)
            .map_err(|error| format!("invalid factory metadata: {error}"))?;
        let definition: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid factory metadata: {error}"))?;
        definition.check_name(expected)?;
        definition.check_arguments(arguments)?;
        Ok(definition)
    }

    /// Validates standard metadata and all argument constraints before provider initialization.
    ///
    /// # Errors
    /// Rejects malformed or mismatched metadata, invalid limits, and invalid arguments or schemas.
    pub fn parse(bytes: &[u8], expected: &str, arguments: &Value) -> Result<Self, SetupError> {
        Self::parse_inner(bytes, expected, arguments).map_err(SetupError::Definition)
    }
}

#[cfg(test)]
#[path = "definition_tests.rs"]
mod tests;
