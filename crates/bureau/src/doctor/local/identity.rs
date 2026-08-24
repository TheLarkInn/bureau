//! Per-credential forge identity status.
//!
//! The results are injected: the command layer performs the read-only
//! verification and hands the answers in, so these effects keep their
//! promise never to contact a forge themselves.

use std::collections::BTreeMap;

use super::LocalEffects;
use crate::config::{CONFIG_CREDENTIAL, Repo, config_forge, config_repo};
use crate::doctor::{Observation, Status};
use crate::setup::Settings;

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

/// One credential a caller may verify.
///
/// Carries the reference, the identity its declaration names, and the
/// repo whose forge answers for it. That repo is a registered one,
/// except for the reserved config credential, whose entry the config
/// remote implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityTarget {
    /// Credential reference.
    pub reference: String,
    /// Identity declared in settings, when declared.
    pub declared: Option<String>,
    /// The repo whose forge the credential authenticates against.
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

/// What makes two checks the same check: one value offered to one host.
/// A reference two repos name on two hosts is two questions, exactly as
/// a run asks them.
fn pair(target: &IdentityTarget) -> (String, String) {
    (target.reference.clone(), target.repo.forge_host())
}

/// The reserved config credential, when settings declare an identity
/// for it. The runner clones the reviewed config remote with it before
/// any registry exists, so that remote is the one host authorized to
/// answer — which is why `validate` leaves the declaration exempt from
/// the orphan rule rather than unenforced.
fn config_target(settings: &Settings) -> Option<IdentityTarget> {
    let identity = settings.declared_identity(CONFIG_CREDENTIAL)?;
    let remote = settings.config.remote();
    Some(IdentityTarget {
        reference: CONFIG_CREDENTIAL.to_owned(),
        declared: Some(identity.to_owned()),
        repo: config_repo(remote, config_forge(remote)),
    })
}

impl LocalEffects {
    pub(super) fn inspect_identity(&self) -> Observation {
        observation(&self.identities)
    }

    /// One target per credential reference a registered repo names, on
    /// each host those repos point at. Empty without a cached config,
    /// because then no repo is known.
    fn registry_targets(
        &self,
        declared: &BTreeMap<String, String>,
    ) -> BTreeMap<(String, String), IdentityTarget> {
        let Ok(Some(config)) = self.cached_config() else {
            return BTreeMap::new();
        };
        let mut targets = BTreeMap::new();
        for repo in config.repos.values() {
            let found = target(repo.credential.clone(), declared, repo);
            targets.entry(pair(&found)).or_insert(found);
        }
        targets
    }

    /// The credentials a caller can verify read-only before the machine
    /// runs: one per reference-and-host a registered repo names, plus
    /// the reserved config credential when it declares an identity,
    /// each with whatever identity its declaration requires. A repo
    /// already naming that reference on the config remote's own host
    /// keeps its entry, so nothing is checked twice. Empty without
    /// settings, because then there is nothing to check.
    #[must_use]
    pub fn identity_targets(&self) -> Vec<IdentityTarget> {
        let Some(settings) = self.local_settings() else {
            return Vec::new();
        };
        let mut targets = self.registry_targets(&settings.declared_identities());
        if let Some(config) = config_target(settings) {
            targets.entry(pair(&config)).or_insert(config);
        }
        targets.into_values().collect()
    }
}
