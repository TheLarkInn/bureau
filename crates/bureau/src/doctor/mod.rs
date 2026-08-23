//! Read-only, offline diagnostics over injected local observations.

mod local;
pub(crate) mod state_db;

pub use bureau_lifecycle::doctor::{
    Area, Diagnostic, Effects, Error, Machine, Observation, Report, Status, run,
};
pub use local::{CredentialIdentity, IdentityTarget, LocalEffects};
pub use state_db::active_lease_count;

/// A read-only inspection of local durable state failed.
#[derive(Debug, thiserror::Error)]
pub enum InspectionError {
    /// The local state database could not be read.
    #[error("{0}")]
    StateDb(String),
    /// A run's durable history could not be replayed.
    #[error("{0}")]
    Replay(String),
}

/// Replays a run without truncating or otherwise mutating durable history.
///
/// # Errors
/// Rejects malformed event history.
pub fn replay_run_read_only(
    directory: &std::path::Path,
) -> Result<crate::runlog::RunState, InspectionError> {
    local::replay_run_read_only(directory).map_err(InspectionError::Replay)
}
