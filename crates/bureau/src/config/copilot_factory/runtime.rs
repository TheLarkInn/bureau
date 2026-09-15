//! SDK capability contract and structural identity of a preprovisioned Copilot runtime.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{object::Object, validation};

const SDK_FACTORY_V1: &str = "copilot-sdk-factory-v1";

/// Bureau's SDK capability contract, not an upstream release or version mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum CopilotFactoryProfile {
    /// The SDK factory capabilities required by Bureau's first supported contract.
    SdkFactoryV1,
}

impl TryFrom<String> for CopilotFactoryProfile {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value == SDK_FACTORY_V1 {
            Ok(Self::SdkFactoryV1)
        } else {
            Err("profile must be Bureau's SDK capability contract copilot-sdk-factory-v1")
        }
    }
}

impl From<CopilotFactoryProfile> for String {
    fn from(profile: CopilotFactoryProfile) -> Self {
        match profile {
            CopilotFactoryProfile::SdkFactoryV1 => SDK_FACTORY_V1.to_owned(),
        }
    }
}

fn check_path(path: &Path, field: &str, allow_current: bool, errors: &mut Vec<String>) {
    validation::check_relative(
        path,
        &format!("copilot_factory.runtime.{field}"),
        allow_current,
        errors,
    );
}

/// Fields of a self-contained runtime and SDK tree, decoded through `CopilotFactoryRuntime`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename = "CopilotFactoryRuntime")]
pub struct CopilotFactoryRuntimeFields {
    /// Bureau's required SDK capability contract.
    pub profile: CopilotFactoryProfile,
    /// Absolute path to the preprovisioned runtime and SDK tree.
    pub directory: PathBuf,
    /// Reviewed runtime tree digest in canonical `tree-sha256:<hex>` form.
    pub digest: String,
    /// Exact expected SDK `connect.version`; no release inference or normalization.
    pub version: String,
    /// Safe relative path to the actual host image (`process.execPath`).
    pub executable: PathBuf,
    /// Safe relative CLI JavaScript entrypoint; omitted for an embedded CLI image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<PathBuf>,
    /// Safe relative CLI distribution directory; `.` names the runtime root.
    pub dist: PathBuf,
}

impl CopilotFactoryRuntimeFields {
    fn check_version(&self, errors: &mut Vec<String>) {
        if self.version.trim().is_empty() {
            errors.push("`copilot_factory.runtime.version` must be nonempty".to_owned());
        }
    }

    fn check_paths(&self, errors: &mut Vec<String>) {
        check_path(&self.executable, "executable", false, errors);
        check_path(&self.dist, "dist", true, errors);
        if let Some(cli) = &self.cli {
            check_path(cli, "cli", false, errors);
        }
    }

    pub(super) fn check(&self, errors: &mut Vec<String>) {
        validation::check_absolute(&self.directory, "copilot_factory.runtime.directory", errors);
        validation::check_digest(&self.digest, "copilot_factory.runtime.digest", errors);
        self.check_version(errors);
        self.check_paths(errors);
    }
}

/// A self-contained runtime and SDK tree with strict map-only decoding.
pub type CopilotFactoryRuntime = Object<CopilotFactoryRuntimeFields>;
