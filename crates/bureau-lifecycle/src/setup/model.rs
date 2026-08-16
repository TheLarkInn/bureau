use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Non-secret local settings stored under `BUREAU_HOME`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Committed configuration source.
    pub config: ConfigSource,
    /// Resolution source for each credential reference.
    pub credentials: BTreeMap<String, CredentialSource>,
    /// User-global plugin choice.
    #[serde(default)]
    pub plugin: PluginSettings,
    /// Optional migration source.
    #[serde(default)]
    pub migration: MigrationSettings,
}

/// Location of reviewed configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigSource {
    /// Configuration is committed under `.bureau/` in one work repository.
    SingleRepository {
        /// Work repository remote.
        remote: String,
        /// Committed ref to reconcile.
        reference: String,
    },
    /// Configuration is committed at the root of a separate repository.
    SeparateRepository {
        /// Configuration repository remote.
        remote: String,
        /// Committed ref to reconcile.
        reference: String,
    },
}

impl ConfigSource {
    /// Repository remote containing the configuration.
    #[must_use]
    pub fn remote(&self) -> &str {
        match self {
            Self::SingleRepository { remote, .. } | Self::SeparateRepository { remote, .. } => {
                remote
            }
        }
    }

    /// Committed ref to reconcile.
    #[must_use]
    pub fn reference(&self) -> &str {
        match self {
            Self::SingleRepository { reference, .. }
            | Self::SeparateRepository { reference, .. } => reference,
        }
    }

    /// Directory holding configuration inside the committed repository.
    #[must_use]
    pub fn subdirectory(&self) -> &Path {
        match self {
            Self::SingleRepository { .. } => Path::new(".bureau"),
            Self::SeparateRepository { .. } => Path::new("."),
        }
    }
}

/// A secret-free credential value reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialSource {
    /// Read the value from one environment variable.
    Environment {
        /// Environment variable name.
        variable: String,
    },
    /// Read a file named after the credential under this directory.
    Directory {
        /// Credential file directory.
        path: PathBuf,
    },
    /// Read the value from this exact file.
    File {
        /// Credential file.
        path: PathBuf,
    },
}

/// User-global plugin behavior.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettings {
    /// Install the bundled plugin for the current user.
    pub install_user_global: bool,
}

/// Import location for an explicit local-state migration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationSettings {
    /// Existing local-state root to import, when requested.
    pub source: Option<PathBuf>,
}

/// How the first pipeline is obtained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FirstPipeline {
    /// Use the deterministic bundled reference configuration.
    Fixed,
    /// Ask the injected authoring effect to create the files.
    AiAuthored {
        /// Human authoring request.
        request: String,
    },
}

/// Inputs required by first-time initialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitRequest {
    /// Local non-secret settings.
    pub settings: Settings,
    /// First pipeline selection.
    pub first_pipeline: FirstPipeline,
}

/// Structured configuration presented, validated, and proposed unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDraft {
    /// Complete config-relative path-to-bytes proposal.
    pub files: BTreeMap<PathBuf, Vec<u8>>,
}

/// Created configuration pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPullRequest {
    /// Forge pull-request identity.
    pub id: String,
}

/// Merge result for the created configuration pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    /// Exact merged commit.
    pub commit: String,
}

/// A committed config source validated at one exact commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedConfig {
    /// Source that was validated.
    pub source: ConfigSource,
    /// Exact validated commit.
    pub commit: String,
}

/// One reconcile pass whose started runs can be awaited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcilePass {
    /// Effect-owned pass identity.
    pub id: String,
}

/// Terminal run outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The run succeeded.
    Success,
    /// The run failed.
    Failure,
    /// The run requires an external action.
    Blocked,
    /// The run found no work.
    NoWork,
}

/// Outcome of one run started by the reconcile pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    /// Durable run identity.
    pub run_id: String,
    /// Terminal result.
    pub outcome: Outcome,
}

/// All outcomes from one completed reconcile pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutcomeSummary {
    /// Runs started by the pass.
    pub runs: Vec<RunSummary>,
}

/// Successful initialization result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    /// Exact reviewed configuration that ran.
    pub config: ValidatedConfig,
    /// Outcomes from the one reconcile pass.
    pub outcomes: OutcomeSummary,
}
