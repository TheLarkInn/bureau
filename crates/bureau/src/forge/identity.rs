//! Verifying who a resolved credential authenticates as, before a run
//! spawns anything (DESIGN.md section 7, layer 0).
//!
//! Presence is structural and resolution already checks it; this is the
//! separate question only the forge answers — whether the value works at
//! all, and whether it works as the declared account.

use std::collections::BTreeMap;

use super::{Error, Forge, Identity};
use crate::process::Secret;

/// A resolved credential the forge will not accept for this run. No
/// variant carries a credential value.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The forge refused the value itself.
    #[error(
        "credential `{reference}` is invalid or expired: the forge rejected it (status {status})"
    )]
    Rejected {
        /// The credential reference that failed.
        reference: String,
        /// Status the forge answered with.
        status: u16,
    },
    /// The forge accepted the value as a different account.
    #[error(
        "credential `{reference}` authenticates as `{observed}`, not the declared identity `{expected}`"
    )]
    Mismatch {
        /// The credential reference that failed.
        reference: String,
        /// Identity declared in settings.
        expected: String,
        /// Identity the forge reported.
        observed: String,
    },
    /// The forge could not say who the credential is.
    #[error("credential `{reference}` could not be verified: {detail}")]
    Unverifiable {
        /// The credential reference that could not be checked.
        reference: String,
        /// Secret-free reason, never a forge response body.
        detail: String,
    },
}

/// A forge failure described without echoing any response body, so no
/// message can carry a value the forge reflected back.
fn detail(error: &Error) -> String {
    match error {
        Error::Http(_) => "the forge could not be reached".to_owned(),
        Error::Api { status, .. } => format!("the forge answered with status {status}"),
        Error::Parse(_) => "the forge answered in an unexpected shape".to_owned(),
        Error::RateLimited { .. } => "the forge rate limit is exhausted".to_owned(),
    }
}

/// Splits "the forge refused this value" from every other failure, so an
/// invalid or expired credential never reads as a transport problem.
fn refused(reference: &str, error: &Error) -> IdentityError {
    match error {
        Error::Api { status, .. } if matches!(status, 401 | 403) => IdentityError::Rejected {
            reference: reference.to_owned(),
            status: *status,
        },
        other => IdentityError::Unverifiable {
            reference: reference.to_owned(),
            detail: detail(other),
        },
    }
}

fn mismatch(reference: &str, expected: &str, observed: &Identity) -> IdentityError {
    IdentityError::Mismatch {
        reference: reference.to_owned(),
        expected: expected.to_owned(),
        observed: observed.account.clone(),
    }
}

/// Checks one resolved credential against the identity its declaration
/// names. `None` comes back when the forge reports no identity at all —
/// the offline fake, unless a test opts in.
///
/// # Errors
/// Rejects a value the forge refuses, a value it cannot answer for, and
/// a value that authenticates as another account.
pub async fn verify(
    forge: &dyn Forge,
    reference: &str,
    credential: &Secret,
    declared: Option<&str>,
) -> Result<Option<Identity>, IdentityError> {
    let observed = forge
        .identity(credential)
        .await
        .map_err(|error| refused(reference, &error))?;
    match (observed, declared) {
        (Some(observed), Some(expected)) if !observed.is(expected) => {
            Err(mismatch(reference, expected, &observed))
        }
        (observed, _) => Ok(observed),
    }
}

/// Checks every credential a run resolved, once, and returns the
/// identities to pin for its life. A reference the forge reports no
/// identity for is absent from the result.
///
/// # Errors
/// Returns the first failing credential, named.
pub async fn verify_all(
    forge: &dyn Forge,
    credentials: &BTreeMap<String, Secret>,
    declared: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, IdentityError> {
    let mut verified = BTreeMap::new();
    for (reference, credential) in credentials {
        let expected = declared.get(reference).map(String::as_str);
        let observed = verify(forge, reference, credential, expected).await?;
        if let Some(identity) = observed {
            verified.insert(reference.clone(), identity.account);
        }
    }
    Ok(verified)
}
