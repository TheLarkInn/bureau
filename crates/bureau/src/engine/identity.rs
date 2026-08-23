//! The run's one identity check.
//!
//! Every credential the run resolved is verified before the worktree is
//! cut — so before any subprocess spawns — and the identities the forge
//! reports are pinned into `run_started`. A resumed run reuses what its
//! log already recorded instead of asking again, so the run keeps the
//! identity it started with even if the underlying source changes.

use super::context::RunCtx;

/// Verifies this run's credentials once, or reuses the pinned result.
///
/// # Errors
/// Returns the secret-free failure to abort the run before spawn.
pub(super) async fn verify(ctx: &mut RunCtx) -> Result<(), String> {
    if ctx.started {
        return Ok(()); // already pinned by the entry that recorded run_started
    }
    let plan = &ctx.plan;
    let verified = crate::forge::identity::verify_all(
        plan.forge.as_ref(),
        &plan.credentials,
        &plan.identities,
    )
    .await
    .map_err(|error| error.to_string())?;
    ctx.verified = verified;
    Ok(())
}
