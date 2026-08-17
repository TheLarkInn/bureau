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
mod usage;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::config::{Role, StepDef};
use crate::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust};
use crate::process::{Secret, SharedLog, SpawnResult};

pub use crate::config::AdapterKind;
pub use usage::{Execution, Usage};

type ExecuteFuture<'a> = Pin<Box<dyn Future<Output = Execution> + Send + 'a>>;

const fn successful(result: &SpawnResult) -> bool {
    matches!(
        (result.outcome, result.exit_code),
        (crate::process::SpawnOutcome::Exited, Some(0))
    )
}

fn missing_result() -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Failure,
        outputs: std::collections::BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: "agent did not publish a result; call `bureau-io.publish_result` before finishing or print a valid v2 StepResult JSON document".to_owned(),
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
        message: tail(result),
    }
}

/// A real/fake agent must return a valid contract result after exit zero.
#[must_use]
pub fn result_from_agent(
    spawned: &SpawnResult,
    published: Option<StepResult>,
    response: &[u8],
) -> StepResult {
    if !successful(spawned) {
        return result_from_spawn(spawned);
    }
    if let Some(result) = published {
        return result;
    }
    StepResult::from_json(response).unwrap_or_else(|_| missing_result())
}

pub(crate) fn failed(message: &str) -> Execution {
    Execution::new(
        StepResult {
            schema: SCHEMA_VERSION.to_owned(),
            outcome: StepOutcome::Failure,
            outputs: std::collections::BTreeMap::new(),
            artifacts: Vec::new(),
            trust: Trust::Derived,
            message: message.to_owned(),
        },
        Usage::unknown("adapter"),
    )
}

pub(crate) fn cancel_path(request: &StepRequest) -> Option<std::path::PathBuf> {
    request
        .worktree
        .parent()
        .map(|parent| parent.join("CANCEL"))
}

/// Runs one agent step through the role's adapter and returns the step's
/// result. Adapter failures are data: they surface as
/// [`StepOutcome::Failure`] with the detail in `message`, never as a
/// panic.
pub async fn execute(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> Execution {
    let future: ExecuteFuture<'_> = match role.adapter {
        AdapterKind::Fake => Box::pin(fake::execute(step, request, timeout, secrets, log)),
        AdapterKind::Copilot => {
            Box::pin(copilot::execute(role, step, request, timeout, secrets, log))
        }
        AdapterKind::Claude => {
            Box::pin(claude::execute(role, step, request, timeout, secrets, log))
        }
    };
    future.await
}
