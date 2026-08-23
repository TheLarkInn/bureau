//! Per-credential forge identity status.
//!
//! The results are injected: the command layer performs the read-only
//! verification and hands the answers in, so these effects keep their
//! promise never to contact a forge themselves.

use std::collections::BTreeMap;

use super::LocalEffects;
use crate::config::Repo;
use crate::doctor::{Observation, Status};

/// One credential's identity check, as the caller observed it. Carries
/// account names and failure detail only — never a credential value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialIdentity {
    reference: String,
    status: Status,
    detail: String,
}

impl CredentialIdentity {
    /// The forge answered, naming this account.
    #[must_use]
    pub fn verified(reference: &str, account: &str) -> Self {
        Self {
            reference: reference.to_owned(),
            status: Status::Ok,
            detail: format!("authenticates as `{account}`"),
        }
    }

    /// The forge could not answer, so nothing was proven either way.
    #[must_use]
    pub fn unchecked(reference: &str, detail: &str) -> Self {
        Self {
            reference: reference.to_owned(),
            status: Status::Warning,
            detail: detail.to_owned(),
        }
    }

    /// The forge refused the value, or answered with another account.
    #[must_use]
    pub fn failed(reference: &str, detail: &str) -> Self {
        Self {
            reference: reference.to_owned(),
            status: Status::Error,
            detail: detail.to_owned(),
        }
    }

    fn line(&self) -> String {
        format!("{}: {}", self.reference, self.detail)
    }
}

/// One credential a caller may verify: the reference, the identity its
/// declaration names, and the repo whose forge answers for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityTarget {
    /// Credential reference.
    pub reference: String,
    /// Identity declared in settings, when declared.
    pub declared: Option<String>,
    /// A registered repo the credential authenticates against.
    pub repo: Repo,
}

const fn code(status: Status) -> &'static str {
    match status {
        Status::Ok => "credential_identity_ok",
        Status::Warning => "credential_identity_unverified",
        Status::Error => "credential_identity_failed",
    }
}

/// Folds the per-credential results into one area observation.
fn observation(results: &[CredentialIdentity]) -> Observation {
    let Some(status) = results.iter().map(|result| result.status).max() else {
        return Observation::new(
            Status::Warning,
            code(Status::Warning),
            "no credential identity was verified",
        );
    };
    let lines: Vec<String> = results.iter().map(CredentialIdentity::line).collect();
    Observation::new(status, code(status), lines.join("; "))
}

fn target(reference: String, declared: &BTreeMap<String, String>, repo: &Repo) -> IdentityTarget {
    IdentityTarget {
        declared: declared.get(&reference).cloned(),
        reference,
        repo: repo.clone(),
    }
}

impl LocalEffects {
    pub(super) fn inspect_identity(&self) -> Observation {
        observation(&self.identities)
    }

    /// The credentials a caller can verify read-only before the machine
    /// runs: one per reference a registered repo names, with whatever
    /// identity its declaration requires. Empty without settings or a
    /// cached config, because then there is nothing to check.
    #[must_use]
    pub fn identity_targets(&self) -> Vec<IdentityTarget> {
        let (Some(settings), Ok(Some(config))) = (self.local_settings(), self.cached_config())
        else {
            return Vec::new();
        };
        let declared = settings.declared_identities();
        let mut targets: BTreeMap<String, IdentityTarget> = BTreeMap::new();
        for repo in config.repos.values() {
            let reference = repo.credential.clone();
            targets
                .entry(reference.clone())
                .or_insert_with(|| target(reference, &declared, repo));
        }
        targets.into_values().collect()
    }
}
