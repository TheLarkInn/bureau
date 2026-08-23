//! The secret-free failures the identity check reports. Every
//! constructor takes the credential *reference* and never the value, so
//! no message can carry a secret or a forge response body.

use super::{Expected, Identity};
use crate::forge::Error;

impl std::fmt::Display for Expected {
    /// The connective a mismatch message reads with; the phrasing lives
    /// beside the only format string it is spliced into.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Declared => "not the declared identity",
            Self::Pinned => "not the identity this run started with",
        })
    }
}

/// A resolved credential the forge will not accept for this run.
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
    #[error("credential `{reference}` authenticates as `{observed}`, {expectation} `{expected}`")]
    Mismatch {
        /// The credential reference that failed.
        reference: String,
        /// Whether the expected identity was declared or pinned.
        expectation: Expected,
        /// Identity the credential had to be.
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

/// Splits "the forge refused this value" from every other failure, so
/// an invalid or expired credential never reads as a transport problem.
///
/// Only 401 is a refusal. GitHub answers 403 to `GET /user` for a valid
/// GitHub App installation token, and the client falls back to a
/// read-only installation call to settle that; a 403 the fallback never
/// confirmed leaves the value unproven, not expired.
pub(super) fn refused(reference: &str, error: &Error) -> IdentityError {
    match error {
        Error::Api { status: 401, .. } => IdentityError::Rejected {
            reference: reference.to_owned(),
            status: 401,
        },
        other => IdentityError::Unverifiable {
            reference: reference.to_owned(),
            detail: detail(other),
        },
    }
}

pub(super) fn mismatch(
    reference: &str,
    expectation: Expected,
    expected: &str,
    observed: &Identity,
) -> IdentityError {
    IdentityError::Mismatch {
        reference: reference.to_owned(),
        expectation,
        expected: expected.to_owned(),
        observed: observed.account.clone(),
    }
}

fn unverifiable(reference: &str, detail: &str) -> IdentityError {
    IdentityError::Unverifiable {
        reference: reference.to_owned(),
        detail: detail.to_owned(),
    }
}

/// A value the forge accepts but will not name cannot prove it is the
/// expected account, so a named expectation fails closed.
pub(super) fn unnamed(reference: &str) -> IdentityError {
    unverifiable(
        reference,
        "the forge accepts the value but names no account for it, so an expected identity cannot be checked",
    )
}

/// No registered repo names this credential, so no host is authorized
/// to be asked about it.
pub(super) fn unauthorized(reference: &str) -> IdentityError {
    unverifiable(
        reference,
        "no registered repo references this credential, so no forge is authorized to verify it",
    )
}

/// Two authorized hosts named different accounts for one value.
pub(super) fn disagreement(reference: &str) -> IdentityError {
    unverifiable(
        reference,
        "the hosts its repos authorize report different accounts for it",
    )
}
