use serde::{Deserialize, Serialize};

/// A required local diagnostic area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// Local home paths, files, directories, and permissions.
    LocalState,
    /// The configured committed config source and local cache.
    ConfigSource,
    /// Registered repositories and their local checkout state.
    Repositories,
    /// Credential reference names and whether each resolves.
    CredentialReferences,
    /// The forge identity each credential authenticates as.
    CredentialIdentity,
    /// Configured adapter binaries and local usability.
    Adapters,
    /// Plugin resolution, activation recovery, and MCP availability.
    PluginsAndMcp,
    /// Interrupted runs, ownership, worktrees, and derived state.
    RecoveryState,
}

impl Area {
    /// Every required check in deterministic display order.
    pub const ALL: [Self; 8] = [
        Self::LocalState,
        Self::ConfigSource,
        Self::Repositories,
        Self::CredentialReferences,
        Self::CredentialIdentity,
        Self::Adapters,
        Self::PluginsAndMcp,
        Self::RecoveryState,
    ];

    /// Human-readable stable area name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalState => "local state",
            Self::ConfigSource => "config source",
            Self::Repositories => "repositories",
            Self::CredentialReferences => "credential references",
            Self::CredentialIdentity => "credential identity",
            Self::Adapters => "adapters",
            Self::PluginsAndMcp => "plugins/MCP",
            Self::RecoveryState => "recovery state",
        }
    }
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The observed area is healthy.
    Ok,
    /// The area needs attention but does not prevent local operation.
    Warning,
    /// The area cannot be used safely.
    Error,
}

impl Status {
    /// Stable human output token.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One read-only effect result before it is assigned to an area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    status: Status,
    code: String,
    message: String,
}

impl Observation {
    /// Creates an observation with a stable machine-readable code.
    #[must_use]
    pub fn new(status: Status, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    pub(super) fn inspection_failed(message: String) -> Self {
        Self::new(Status::Error, "inspection_failed", message)
    }
}

/// One area-qualified diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Area inspected.
    pub area: Area,
    /// Severity.
    pub status: Status,
    /// Stable machine-readable finding code.
    pub code: String,
    /// Human-readable finding without credential values.
    pub message: String,
}

impl Diagnostic {
    pub(super) fn from_observation(area: Area, observation: Observation) -> Self {
        Self {
            area,
            status: observation.status,
            code: observation.code,
            message: observation.message,
        }
    }
}

/// Complete deterministic doctor output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub(super) const fn new(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }

    /// Findings in required check order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Highest severity in the report.
    #[must_use]
    pub fn status(&self) -> Status {
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.status)
            .max()
            .unwrap_or(Status::Ok)
    }
}
