//! `bureau doctor`'s credential identity pass.
//!
//! The doctor's own effects never contact a forge, so the command layer
//! asks each registered repo's forge who its credential is and injects
//! the answers. Every call is a read: nothing is created, changed, or
//! deleted, and no credential value reaches the report.

use bureau::doctor::{CredentialIdentity, IdentityTarget, LocalEffects};
use bureau::forge::identity::{Check, Expected, IdentityError, Reported};
use bureau::setup::Settings;

use crate::cli::prepare;

/// A forge the run could not reach is a warning; a refused or mismatched
/// credential is an error, because those are answers, not outages.
fn failure(reference: &str, error: &IdentityError) -> CredentialIdentity {
    match error {
        IdentityError::Unverifiable { .. } => {
            CredentialIdentity::unchecked(reference, &error.to_string())
        }
        other => CredentialIdentity::failed(reference, &other.to_string()),
    }
}

/// What the forge answered, as one report line. A value accepted
/// without a name proves the credential works and nothing more.
fn reported(reference: &str, answer: Result<Reported, IdentityError>) -> CredentialIdentity {
    match answer {
        Ok(Reported::Account(identity)) => {
            CredentialIdentity::verified(reference, &identity.account)
        }
        Ok(Reported::Unnamed) => CredentialIdentity::unchecked(
            reference,
            "the forge accepts the value but names no account for it",
        ),
        Ok(Reported::Silent) => {
            CredentialIdentity::unchecked(reference, "the forge reported no identity")
        }
        Err(error) => failure(reference, &error),
    }
}

async fn check(settings: &Settings, target: &IdentityTarget) -> CredentialIdentity {
    let reference = target.reference.as_str();
    let secret = match bureau::credential::resolve(settings, reference) {
        Ok(secret) => secret,
        Err(error) => return CredentialIdentity::failed(reference, &error.to_string()),
    };
    let forge = match prepare::repo_forge(&target.repo, secret.clone()) {
        Ok(forge) => forge,
        Err(error) => return CredentialIdentity::unchecked(reference, &error.to_string()),
    };
    let check = Check {
        reference,
        credential: &secret,
        expected: target.declared.as_deref(),
        expectation: Expected::Declared,
    };
    reported(
        reference,
        bureau::forge::identity::verify(forge.as_ref(), &check).await,
    )
}

/// Verifies every credential a registered repo references, read-only.
pub(super) async fn verify(effects: &LocalEffects) -> Vec<CredentialIdentity> {
    let Some(settings) = effects.local_settings() else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for target in effects.identity_targets() {
        results.push(check(settings, &target).await);
    }
    results
}
