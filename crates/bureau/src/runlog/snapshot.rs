//! Immutable inputs needed to reconstruct a run after restart.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use bureau_plugin::PluginSource;

use crate::config::{Assignment, Pipeline, Repo, Role};
use crate::forge::Item;

/// Committed configuration identity, populated by committed-source runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSource {
    /// Git remote URL.
    pub remote: String,
    /// Configured branch/ref.
    pub reference: String,
    /// Exact activated commit.
    pub commit: String,
}

/// Everything serializable from a run plan; credentials are references only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSnapshot {
    /// Run id.
    pub run_id: String,
    /// Standing assignment.
    pub assignment: Assignment,
    /// Pinned pipeline.
    pub pipeline: Pipeline,
    /// Pinned role definitions.
    pub roles: BTreeMap<String, Role>,
    /// Pinned repo registry entries; secret values are absent.
    pub repos: BTreeMap<String, Repo>,
    /// Claimed work-item body and trust.
    pub item: Item,
    /// Committed config identity when available.
    #[serde(default)]
    pub config_source: Option<ConfigSource>,
    /// Selected plugin identities keyed by plugin name.
    #[serde(default)]
    pub plugin_sources: BTreeMap<String, PluginSource>,
    /// Pinned direct-agent bytes keyed by role name.
    #[serde(default)]
    pub direct_agents: BTreeMap<String, Vec<u8>>,
}
