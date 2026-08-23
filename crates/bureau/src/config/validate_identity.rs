//! The settings-side identity rule, checked in the same accumulate-all
//! pass as every committed-config rule.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::{Config, push};
use crate::ConfigError;

/// The reserved credential reference: the one the runner uses to clone
/// the reviewed config repo itself. `repos.yaml` never names it,
/// because it is not a repo the pipeline works in.
pub const CONFIG_CREDENTIAL: &str = "config";

/// Whether a declared identity names a credential nothing can exercise.
/// The reserved config reference is exempt: it is used by the runner
/// itself, not by an entry in the registry.
fn orphaned(reference: &str, referenced: &BTreeSet<&str>) -> bool {
    reference != CONFIG_CREDENTIAL && !referenced.contains(reference)
}

/// Declared credential identities no repo can ever exercise.
///
/// The identity belongs to the credential, so it is declared in local
/// settings rather than in `repos.yaml`; a declaration for a reference
/// neither the registry nor the runner itself names is a rule nothing
/// enforces.
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
        if orphaned(reference, &referenced) {
            let message = format!(
                "credential `{reference}`: `identity` is declared but no repo references this credential"
            );
            push(&mut errors, PathBuf::from("settings.yaml"), message);
        }
    }
    errors
}
