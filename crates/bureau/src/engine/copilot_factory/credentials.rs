//! Model access uses the reviewed credential reference, without an ambient fallback.
//! Forge credentials retain the ordinary launcher's independent grant decision.

use crate::config::CopilotFactory;
use crate::process::{Secret, SpawnRequest};

use super::super::context::RunCtx;

pub(super) const MODEL_TOKEN_ENV: &str = "COPILOT_GITHUB_TOKEN";

// The CLI auth flag only reads the variable; child filtering needs this registration.
pub(super) fn configure(request: &mut SpawnRequest, token: &Secret) {
    if !request.secrets.contains(token) {
        request.secrets.push(token.clone());
    }
    // GH_TOKEN is already grant-filtered; equal values do not change that authorization.
    for name in ["GITHUB_TOKEN", "GITHUB_COPILOT_GITHUB_TOKEN"] {
        request.env.remove(name);
    }
    request.argv.extend(
        [
            "--auth-token-env",
            MODEL_TOKEN_ENV,
            "--no-auto-login",
            "--secret-env-vars=COPILOT_GITHUB_TOKEN",
        ]
        .map(str::to_owned),
    );
    request
        .env
        .insert(MODEL_TOKEN_ENV.to_owned(), token.expose().to_owned());
}

fn resolved<'a>(ctx: &'a RunCtx, reference: &str) -> Result<&'a Secret, String> {
    if reference.trim().is_empty() {
        return Err("local Copilot factory has no approved model credential reference".into());
    }
    ctx.plan.credentials.get(reference).ok_or_else(|| {
        format!("approved Copilot model credential `{reference}` is unavailable; no ambient fallback is allowed")
    })
}

pub(super) fn resolve(ctx: &RunCtx, factory: &CopilotFactory) -> Result<Secret, String> {
    let token = resolved(ctx, &factory.model_credential)?;
    if token.expose().trim().is_empty() {
        return Err("approved Copilot model credential resolved to an empty value".into());
    }
    Ok(token.clone())
}

pub(super) fn same_reference(
    saved: &CopilotFactory,
    requested: &CopilotFactory,
) -> Result<(), String> {
    if saved.model_credential != requested.model_credential {
        return Err("factory model credential reference differs from its durable intent".into());
    }
    Ok(())
}
