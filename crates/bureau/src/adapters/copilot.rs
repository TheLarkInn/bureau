//! The `copilot` adapter: runs the GitHub Copilot CLI as the agent.
//!
//! The agent file a developer invokes locally as `/plugin:agent` runs
//! unmodified in automation (DESIGN.md section 6): the adapter passes the
//! reference through, and mirrors the role's permissions in argv
//! (`--allow-tool` / `--deny-tool`) where the CLI can enforce them.

use std::time::Duration;

use crate::config::{Role, StepDef};
use crate::contract::{StepRequest, StepResult};
use crate::process::{Secret, SharedLog, SpawnRequest};

/// The adapter's working binary name.
pub const BINARY: &str = "copilot";

/// Builds the layer-0 request for a `copilot` step: the step contract
/// JSON on stdin, credentials in env, permission flags in argv.
///
/// `credentials` are the secrets the role's permissions grant, already
/// resolved; every value is also present in `secrets` for scrubbing.
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
        "adapters-real: argv = copilot -p --agent <ref> ... ; env = credential vars (GH_TOKEN); stdin = request JSON; timeout from step"
    )
}

/// Runs a `copilot` step and derives the step result.
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

/// Default per-step timeout when the pipeline does not set one.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1800);
