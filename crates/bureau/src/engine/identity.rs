//! The run's one identity check.
//!
//! Every credential the run resolved is verified before the worktree is
//! cut — so before any subprocess spawns — against the forges its own
//! repos authorize, and the identities they report are pinned into
//! `run_started`. A resumed run re-resolves and re-checks the same
//! credentials against what its log already pinned, so a rotated value
//! that now belongs to another account aborts the resume before its
//! next step instead of running as somebody else.

use std::collections::BTreeMap;

use super::context::RunCtx;
use crate::forge::identity::{Expected, verify_all};

/// What each credential must match: on a fresh run the identity local
/// settings declare, on every resume the identity `run_started` pinned.
fn expectation(ctx: &RunCtx) -> (Expected, BTreeMap<String, String>) {
    if ctx.started {
        (Expected::Pinned, ctx.pinned.clone())
    } else {
        (Expected::Declared, ctx.plan.identities.clone())
    }
}

/// Verifies this run's credentials, fresh or resumed.
///
/// # Errors
/// Returns the secret-free failure to abort the run before spawn.
pub(super) async fn verify(ctx: &mut RunCtx) -> Result<(), String> {
    let (expectation, expected) = expectation(ctx);
    let plan = &ctx.plan;
    let verified = verify_all(
        &plan.identity_forges,
        &plan.credentials,
        &expected,
        expectation,
    )
    .await
    .map_err(|error| error.to_string())?;
    ctx.verified = verified;
    Ok(())
}
