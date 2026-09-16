use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::{AutomationId, Error, Repository};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: Option<i64>,
    pub login: Option<String>,
    pub node_id: Option<String>,
    pub url: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisabledState {
    pub reason: String,
    pub disabled_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub types: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn triggers<'de, D>(deserializer: D) -> Result<BTreeMap<String, Trigger>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<BTreeMap<String, Trigger>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub id: AutomationId,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: User,
    pub repository: Option<Repository>,
    pub prompt: Option<String>,
    pub disabled_state: Option<DisabledState>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, deserialize_with = "triggers")]
    pub triggers: BTreeMap<String, Trigger>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchEvent {
    Manual,
    Interval,
}

impl DispatchEvent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Interval => "interval",
        }
    }
}

fn trigger_event(triggers: &BTreeMap<String, Trigger>) -> Result<DispatchEvent, Error> {
    if triggers.is_empty() {
        return Ok(DispatchEvent::Manual);
    }
    if triggers.len() == 1
        && triggers
            .keys()
            .any(|key| matches!(key.as_str(), "interval" | "schedule"))
    {
        return Ok(DispatchEvent::Interval);
    }
    Err(Error::Unsupported(
        "Run now requires no triggers or exactly one interval/schedule trigger".to_owned(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    pub id: AutomationId,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: User,
    pub repository: Option<Repository>,
    pub disabled_state: Option<DisabledState>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub require_actor_write_permission: bool,
    #[serde(default, deserialize_with = "triggers")]
    pub triggers: BTreeMap<String, Trigger>,
    pub tools: Option<Vec<String>>,
    pub permissions: Option<BTreeMap<String, String>>,
    pub github_mcp_toolsets: Option<Vec<String>>,
    pub mcp_servers: Option<Vec<McpServer>>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Definition {
    /// The HTTP client's event for one explicit submission.
    ///
    /// # Errors
    /// Rejects disabled automations and unsupported trigger arrangements.
    pub fn dispatch_event(&self) -> Result<DispatchEvent, Error> {
        if self.disabled || self.disabled_state.is_some() {
            return Err(Error::Unsupported(
                "disabled automations cannot be dispatched by Bureau".to_owned(),
            ));
        }
        trigger_event(&self.triggers)
    }
}
