//! Durable factory-context data shared by the engine and run log.
//! No source discovery, filesystem access, or engine state belongs here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use bureau_plugin::PluginSource;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Literal native custom-agent input, not a mutable Markdown reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomAgent {
    pub name: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedPlugin {
    pub source: PluginSource,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceIdentity {
    Inline,
    Plugin { plugin: String, path: PathBuf },
}

/// Known resource identities; presence alone is not permission or startup approval.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedCatalog {
    pub agents: BTreeMap<String, ResourceIdentity>,
    pub skills: BTreeMap<String, ResourceIdentity>,
    pub commands: BTreeMap<String, ResourceIdentity>,
    pub agent_skills: BTreeMap<String, BTreeSet<String>>,
    /// Protocol server names and unqualified MCP tool names, not guessed native
    /// flattened invocation aliases. Bureau supplies this server on create/resume.
    pub mcp_servers: BTreeMap<String, BTreeSet<String>>,
    /// Plugin owners of the exact bundled bureau-io declaration. The caller must
    /// bind their contribution to the controlled root server, not PATH.
    pub bureau_io_plugins: BTreeSet<String>,
}

/// Durably record this value before SDK initialization. It contains no credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedContext {
    pub worktree: PathBuf,
    pub private_root: PathBuf,
    pub plugins: Vec<PinnedPlugin>,
    pub selected_agent: String,
    pub custom_agent: Option<CustomAgent>,
    pub expected_catalog: ExpectedCatalog,
}

impl PinnedContext {
    /// Explicit preprovisioned plugin roots for `session.create`, never resume.
    #[must_use]
    pub fn plugin_directories(&self) -> Vec<PathBuf> {
        self.plugins
            .iter()
            .map(|plugin| plugin.directory.clone())
            .collect()
    }
}
