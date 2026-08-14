//! The `claude` adapter: runs the Anthropic Claude Code CLI as the agent.
//!
//! Same contract as `copilot`: the agent file runs unmodified, the role's
//! permissions are mirrored in argv (`--allowedTools` / --deny
//! equivalents), and credentials arrive via env only.

use crate::config::{Role, StepDef};
use crate::contract::{StepRequest, StepResult};
use crate::process::{Secret, SharedLog, SpawnRequest};

/// The adapter's working binary name.
pub const BINARY: &str = "claude";

/// Builds the layer-0 request for a `claude` step.
#[must_use]
pub fn spawn_request(
    _role: &Role,
    _step: &StepDef,
    request: &StepRequest,
    _secrets: Vec<Secret>,
    _log: Option<SharedLog>,
) -> SpawnRequest {
    let _ = request;
    todo!(
        "adapters-real: argv = claude -p ... ; env = credential vars (ANTHROPIC_API_KEY / oauth); stdin = request JSON"
    )
}

/// Runs a `claude` step and derives the step result.
#[must_use]
pub async fn execute(
    _role: &Role,
    _step: &StepDef,
    _request: &StepRequest,
    _secrets: Vec<Secret>,
    _log: Option<SharedLog>,
) -> StepResult {
    tokio::task::yield_now().await;
    todo!("adapters-real: spawn, then result_from_spawn")
}
