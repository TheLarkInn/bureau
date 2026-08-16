//! Read-only, offline diagnostics over injected local observations.

mod local;
pub(crate) mod state_db;

pub use bureau_lifecycle::doctor::*;
pub use local::LocalEffects;
pub use state_db::active_lease_count;

/// Replays a run without truncating or otherwise mutating durable history.
///
/// # Errors
/// Rejects malformed event history.
pub fn replay_run_read_only(
    directory: &std::path::Path,
) -> Result<crate::runlog::RunState, String> {
    local::replay_run_read_only(directory)
}
