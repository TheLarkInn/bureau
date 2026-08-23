//! Verifying who a resolved credential authenticates as, before a run
//! spawns anything (DESIGN.md section 7, layer 0).
//!
//! Presence is structural and resolution already checks it; this is the
//! separate question only the forge answers — whether the value works at
//! all, and whether it works as the expected account.
//!
//! A value is only ever offered to a host a registered repo naming that
//! credential points at: [`Authorizations`] carries one client per such
//! host, and a reference with no client is sent nowhere at all.

mod error;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use error::IdentityError;

use super::{Forge, Identity};
use crate::process::Secret;

/// What a forge reports about one resolved credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reported {
    /// The forge names the account the credential authenticates as.
    Account(Identity),
    /// The forge accepts the value but names no account for it: a
    /// GitHub App installation token has no account of its own.
    Unnamed,
    /// The forge reports nothing about identity at all — the offline
    /// fake, unless a test opts in.
    Silent,
}

/// Which identity a credential must match, and so which failure a
/// mismatch is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// The identity local settings declare for the credential.
    Declared,
    /// The identity this run's own `run_started` pinned.
    Pinned,
}

/// One host authorized to answer for a credential, and the client that
/// reaches it. The host is the forge kind and API root a registered
/// repo naming that credential implies.
#[derive(Clone)]
pub struct Authorized {
    /// Forge kind and API root, as the repo implies them.
    pub host: String,
    /// The client addressing that host.
    pub client: Arc<dyn Forge>,
}

/// The hosts authorized to answer for each credential reference: one
/// entry per distinct host a registered repo naming that credential
/// points at. Runtime only — no client enters a run log.
pub type Authorizations = BTreeMap<String, Vec<Authorized>>;

/// One credential to check: the value, and the identity it must be.
pub struct Check<'a> {
    /// The credential reference, named in every failure.
    pub reference: &'a str,
    /// The resolved value, never echoed into an error.
    pub credential: &'a Secret,
    /// The account it must authenticate as, when one is expected.
    pub expected: Option<&'a str>,
    /// Where that expectation came from.
    pub expectation: Expected,
}

/// Whether an accepted credential satisfies the expectation. A forge
/// that reports nothing stays permissive; one that accepts a value it
/// will not name cannot satisfy a name.
fn matched(check: &Check<'_>, reported: Reported) -> Result<Reported, IdentityError> {
    let Some(expected) = check.expected else {
        return Ok(reported);
    };
    match &reported {
        Reported::Account(observed) if !observed.is(expected) => Err(error::mismatch(
            check.reference,
            check.expectation,
            expected,
            observed,
        )),
        Reported::Unnamed => Err(error::unnamed(check.reference)),
        _ => Ok(reported),
    }
}

/// Checks one resolved credential against one authorized forge client.
///
/// # Errors
/// Rejects a value the forge refuses, a value it cannot answer for, and
/// a value that authenticates as another account.
pub async fn verify(forge: &dyn Forge, check: &Check<'_>) -> Result<Reported, IdentityError> {
    let reported = forge
        .identity(check.credential)
        .await
        .map_err(|error| error::refused(check.reference, &error))?;
    matched(check, reported)
}

/// The verdict when no repo authorizes any host for this credential:
/// an expected identity fails closed rather than send the value to a
/// host nothing named, and an unexpected one stays unchecked.
fn unauthorized(check: &Check<'_>) -> Result<Option<Identity>, IdentityError> {
    match check.expected {
        Some(_) => Err(error::unauthorized(check.reference)),
        None => Ok(None),
    }
}

/// Folds one more host's answer into what earlier hosts reported. Two
/// hosts naming different accounts for one value leave the run no
/// single identity to pin, so it fails closed instead of choosing.
fn narrow(
    check: &Check<'_>,
    seen: Option<Identity>,
    reported: Reported,
) -> Result<Option<Identity>, IdentityError> {
    let Reported::Account(observed) = reported else {
        return Ok(seen);
    };
    match seen {
        Some(first) if !observed.is(&first.account) => Err(error::disagreement(check.reference)),
        Some(first) => Ok(Some(first)),
        None => Ok(Some(observed)),
    }
}

/// Checks one credential against every host its own repos authorize.
async fn verify_one(
    hosts: &[Authorized],
    check: &Check<'_>,
) -> Result<Option<Identity>, IdentityError> {
    if hosts.is_empty() {
        return unauthorized(check);
    }
    let mut seen = None;
    for host in hosts {
        seen = narrow(check, seen, verify(host.client.as_ref(), check).await?)?;
    }
    Ok(seen)
}

/// Checks every credential a run resolved, once, against the hosts its
/// own repos authorize, and returns the identities to pin for the run's
/// life. A reference no host named an account for is absent.
///
/// # Errors
/// Returns the first failing credential, named.
pub async fn verify_all(
    authorized: &Authorizations,
    credentials: &BTreeMap<String, Secret>,
    expected: &BTreeMap<String, String>,
    expectation: Expected,
) -> Result<BTreeMap<String, String>, IdentityError> {
    let mut verified = BTreeMap::new();
    for (reference, credential) in credentials {
        let check = Check {
            reference,
            credential,
            expected: expected.get(reference).map(String::as_str),
            expectation,
        };
        let hosts = authorized.get(reference).map(Vec::as_slice);
        if let Some(identity) = verify_one(hosts.unwrap_or_default(), &check).await? {
            verified.insert(reference.clone(), identity.account);
        }
    }
    Ok(verified)
}
