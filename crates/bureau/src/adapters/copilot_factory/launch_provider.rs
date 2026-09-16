//! The real pre-initialization SDK extension launch-provider boundary.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use super::SetupError;
use super::artifacts::Artifacts;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    id: String,
    name: String,
    module_path: PathBuf,
    source: String,
}

impl Candidate {
    fn matches(&self, artifacts: &Artifacts) -> bool {
        self.source == "session"
            && self.id == artifacts.identity.runtime_extension_id
            && self.name == artifacts.identity.runtime_extension_name
    }

    fn check_path(&self, artifacts: &Artifacts) -> Result<(), String> {
        let actual = std::fs::canonicalize(&self.module_path)
            .map_err(|error| format!("resolving factory provider entrypoint: {error}"))?;
        if actual == artifacts.entrypoint() {
            Ok(())
        } else {
            Err("factory provider entrypoint does not match its approved session copy".to_owned())
        }
    }
}

fn profile(artifacts: &Artifacts, request_file: &Path, agent: &str) -> Value {
    json!({
        "launch": {
            "executable": artifacts.launch.executable,
            "args": [artifacts.launch.bootstrap],
            "env": {
                "COPILOT_CLI_DIST_DIR": artifacts.launch.dist,
                "EXTENSION_PATH": artifacts.entrypoint(),
                "BUREAU_FACTORY_REQUEST": request_file,
                "BUREAU_FACTORY_AGENT": agent
            }
        }
    })
}

fn resolve_inner(
    artifacts: &Artifacts,
    parameters: &Value,
    request_file: &Path,
    agent: &str,
) -> Result<Value, String> {
    if !parameters.is_object() {
        return Err("SDK extension launch callback must be an object".to_owned());
    }
    let candidate: Candidate = serde_json::from_value(parameters.clone())
        .map_err(|error| format!("invalid SDK extension launch callback: {error}"))?;
    if !candidate.matches(artifacts) {
        return Ok(json!({}));
    }
    artifacts.verify()?;
    candidate.check_path(artifacts)?;
    Ok(profile(artifacts, request_file, agent))
}

/// Resolves only the approved session provider, before the runtime can start its process.
///
/// An empty object denies every other candidate without native fallback. The root
/// process owns the complete environment; profile env is only an overlay.
///
/// # Errors
/// Rejects malformed callbacks and changed/missing approved code or entrypoints.
pub fn resolve(
    artifacts: &Artifacts,
    parameters: &Value,
    request_file: &Path,
    agent: &str,
) -> Result<Value, SetupError> {
    resolve_inner(artifacts, parameters, request_file, agent).map_err(SetupError::Launch)
}
