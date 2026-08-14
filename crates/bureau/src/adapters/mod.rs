//! One agent CLI integration is an adapter (DESIGN.md section 2).
//!
//! Adapters are how a role's agent — an agent file authored in a plugin,
//! referenced unmodified — gets executed. The `fake` adapter replays
//! recorded transcripts, which is what makes every layer above testable
//! offline, deterministically, in CI. Real adapters (`copilot`, `claude`)
//! gain a `record` mode that writes those transcripts.

pub mod claude;
pub mod copilot;
pub mod fake;
pub(crate) mod real;

use serde::{Deserialize, Serialize};

use std::future::Future;
use std::pin::Pin;

use crate::config::{Role, StepDef};
use crate::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust};
use crate::process::{Secret, SharedLog, SpawnResult};

type ExecuteFuture<'a> = Pin<Box<dyn Future<Output = StepResult> + Send + 'a>>;

/// Runs one agent step through the role's adapter and returns the step's
/// result. Adapter failures are data: they surface as
/// [`StepOutcome::Failure`] with the detail in `message`, never as a
/// panic.
pub async fn execute(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> StepResult {
    let future: ExecuteFuture<'_> = match role.adapter {
        AdapterKind::Fake => Box::pin(fake::execute(step, request, log)),
        AdapterKind::Copilot => Box::pin(copilot::execute(role, step, request, secrets, log)),
        AdapterKind::Claude => Box::pin(claude::execute(role, step, request, secrets, log)),
    };
    future.await
}

/// Derives a step result from a captured subprocess.
///
/// If the process emitted a valid contract document on stdout, that
/// document wins. Otherwise the outcome is derived from how the process
/// ended: exit 0 is `Success`, anything else is `Failure`, with the tail
/// of the captured (already scrubbed) output as the message.
#[must_use]
pub fn result_from_spawn(result: &SpawnResult) -> StepResult {
    if let Ok(parsed) = StepResult::from_json(&result.stdout) {
        return parsed;
    }
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: outcome_of(result),
        outputs: std::collections::BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        cost_usd: 0.0,
        message: tail(result),
    }
}

const fn outcome_of(result: &SpawnResult) -> StepOutcome {
    use crate::process::SpawnOutcome;
    match (result.outcome, result.exit_code) {
        (SpawnOutcome::Exited, Some(0)) => StepOutcome::Success,
        _ => StepOutcome::Failure,
    }
}

fn tail(result: &SpawnResult) -> String {
    let text = String::from_utf8_lossy(&result.stderr);
    let text = text.trim();
    let start = text.len().saturating_sub(500);
    text.get(start..).unwrap_or(text).to_owned()
}

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
