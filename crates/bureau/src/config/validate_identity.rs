//! The settings-side identity rule, checked in the same accumulate-all
//! pass as every committed-config rule.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::{Config, push};
use crate::ConfigError;

/// Declared credential identities no repo can ever exercise.
///
/// The identity belongs to the credential, so it is declared in local
/// settings rather than in `repos.yaml`; a declaration for a reference
/// the registry never names is a rule nothing enforces.
#[must_use]
pub fn validate_identities(
    config: &Config,
    identities: &BTreeMap<String, String>,
) -> Vec<ConfigError> {
    let referenced: BTreeSet<&str> = config
        .repos
        .values()
        .map(|repo| repo.credential.as_str())
        .collect();
    let mut errors = Vec::new();
    for reference in identities.keys() {
        if !referenced.contains(reference.as_str()) {
            let message = format!(
                "credential `{reference}`: `identity` is declared but no repo references this credential"
            );
            push(&mut errors, PathBuf::from("settings.yaml"), message);
        }
    }
    errors
}
