//! Configuration file shapes (DESIGN.md section 6). These live in a
//! separate config repository; PR review of that repo is the entire
//! authorization model. Fields are exact — `deny_unknown_fields` makes
//! adding one a compile-time-visible, reviewed decision.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::contract::Trust;
use crate::forge::ForgeKind;

/// The agent CLI a role runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdapterKind {
    /// GitHub Copilot CLI.
    Copilot,
    /// Anthropic Claude Code.
    Claude,
    /// Replays a recorded transcript; the test seam for every layer.
    Fake,
}

/// A config item whose `name` field must match its file stem.
pub trait Named {
    /// The declared name.
    fn name(&self) -> &str;
}

/// A credential grant, checked before spawn (DESIGN.md section 10). The
/// flat list stays under 15 entries; each maps to a concrete credential
/// that layer 0 will or will not inject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    /// Read repository contents.
    #[serde(rename = "repo:read")]
    RepoRead,
    /// Write repository contents locally.
    #[serde(rename = "repo:write")]
    RepoWrite,
    /// Push branches.
    #[serde(rename = "repo:push")]
    RepoPush,
    /// Read work items / issues.
    #[serde(rename = "issues:read")]
    IssuesRead,
    /// Comment on or edit work items / issues.
    #[serde(rename = "issues:write")]
    IssuesWrite,
    /// Read pull requests.
    #[serde(rename = "pr:read")]
    PrRead,
    /// Open and edit pull requests.
    #[serde(rename = "pr:write")]
    PrWrite,
    /// Review pull requests.
    #[serde(rename = "pr:review")]
    PrReview,
    /// Merge pull requests.
    #[serde(rename = "pr:merge")]
    PrMerge,
    /// Read run state.
    #[serde(rename = "runs:read")]
    RunsRead,
    /// Invoke a model.
    #[serde(rename = "model:invoke")]
    ModelInvoke,
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token = match self {
            Self::RepoRead => "repo:read",
            Self::RepoWrite => "repo:write",
            Self::RepoPush => "repo:push",
            Self::IssuesRead => "issues:read",
            Self::IssuesWrite => "issues:write",
            Self::PrRead => "pr:read",
            Self::PrWrite => "pr:write",
            Self::PrReview => "pr:review",
            Self::PrMerge => "pr:merge",
            Self::RunsRead => "runs:read",
            Self::ModelInvoke => "model:invoke",
        };
        f.write_str(token)
    }
}

/// A per-repo access grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// Read-only context.
    Read,
    /// May open pull requests.
    Pr,
    /// May push.
    Push,
}

impl Access {
    /// Whether the grant allows a branch to land (push or PR).
    #[must_use]
    pub const fn allows_push(self) -> bool {
        matches!(self, Self::Pr | Self::Push)
    }
}

/// One registered repo with its access level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    /// Clone URL.
    pub url: String,
    /// Which forge hosts it.
    pub forge: ForgeKind,
    /// Per-repo grant; a run gets credentials scoped to exactly this.
    pub access: Access,
    /// A credential REFERENCE, resolved at spawn (DESIGN.md section 6).
    pub credential: String,
}

/// `repos.yaml` — the registry, referenced by many assignments, owned by
/// none.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReposFile {
    /// Every repo the runner may touch, by short name.
    pub repos: BTreeMap<String, Repo>,
}

/// Where work items come from. `filter` is a forge-native query string
/// passed through verbatim — WIQL for ADO, search syntax for GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkSource {
    /// Which forge holds the work items.
    pub forge: ForgeKind,
    /// Forge-specific source, e.g. `Odsp/odsp-web`.
    pub source: String,
    /// Forge-native query, opaque to the runner.
    pub filter: String,
    /// Label that admits the item and grades it as maintainer-approved.
    #[serde(default)]
    pub approval_label: Option<String>,
    /// Label added when a run reaches the `abort` terminal.
    #[serde(default)]
    pub abort_label: String,
    /// Label added when a run reaches the `escalate` terminal.
    #[serde(default)]
    pub escalate_label: String,
}

/// Per-assignment limits; a kill switch, not chargeback.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Concurrent runs for this assignment.
    #[serde(default)]
    pub max_concurrent: Option<u32>,
    /// Runs started per hour.
    #[serde(default)]
    pub max_runs_per_hour: Option<u32>,
    /// Runs started per day.
    #[serde(default)]
    pub max_runs_per_day: Option<u32>,
    /// Open PRs this assignment may have at once.
    #[serde(default)]
    pub max_open_prs: Option<u32>,
    /// Daily cost ceiling.
    #[serde(default)]
    pub max_cost_per_day_usd: Option<f64>,
    /// Complete-run deadline; omitted uses the system default.
    #[serde(default)]
    pub max_run_hours: Option<u64>,
}

/// `roles/<name>.yaml` — a reference plus a grant, not a new schema.
///
/// The agent's name, description, instructions, tools, and model live in
/// the plugin/agent file; the role adds only what lives outside the agent
/// process: adapter, credentials (via permissions), and trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// Must match the file stem.
    pub name: String,
    /// A plugin invocation (`/plugin:agent`) or a path to an agent `.md`.
    pub agent: String,
    /// Which agent CLI runs the agent.
    pub adapter: AdapterKind,
    /// Credential grants checked before spawn (DESIGN.md section 10).
    pub permissions: Vec<Permission>,
    /// Minimum input trust this role accepts.
    pub min_trust: Trust,
}

impl Named for Role {
    fn name(&self) -> &str {
        &self.name
    }
}

/// `assignments/<name>.yaml` — the standing arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    /// Must match the file stem.
    pub name: String,
    /// Where work items come from.
    pub work: WorkSource,
    /// Repos the run touches; the first is primary — the branch lands there.
    pub repos: Vec<String>,
    /// Pipeline name (resolved once the engine lands).
    pub pipeline: String,
    /// Role name.
    pub role: String,
    /// Verification command run by a deterministic step.
    pub verify: String,
    /// Every branch carries this prefix so cleanup is one glob.
    pub branch_prefix: String,
    /// Kill switch against a runaway loop (DESIGN.md section 6).
    #[serde(default)]
    pub limits: Limits,
}

impl Assignment {
    /// The repo the branch lands on: the first listed.
    #[must_use]
    pub fn primary_repo(&self) -> Option<&str> {
        self.repos.first().map(String::as_str)
    }
}

impl Named for Assignment {
    fn name(&self) -> &str {
        &self.name
    }
}
