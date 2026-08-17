//! Credential references resolved at spawn time (DESIGN.md section 6).
//!
//! Config names a credential (`credential: ado-main`); the value is never
//! in git. Resolution order: the `BUREAU_CREDENTIAL_<NAME>` environment
//! variable, then a file named `<reference>` under `$BUREAU_CREDENTIALS_DIR`.

use std::path::Path;

use super::secret::Secret;

/// Environment variable prefix for injected credentials: a reference named
/// `ado-main` resolves from `BUREAU_CREDENTIAL_ADO_MAIN`.
pub const ENV_PREFIX: &str = "BUREAU_CREDENTIAL_";

/// Names the directory holding file-backed credentials.
pub const DIR_VAR: &str = "BUREAU_CREDENTIALS_DIR";

/// A credential reference that could not be resolved.
#[derive(Debug, thiserror::Error)]
#[error(
    "missing credential `{0}` (set `{1}` or a file named `{0}` under `$BUREAU_CREDENTIALS_DIR`)"
)]
pub struct CredentialError(
    /// The unresolved reference.
    pub String,
    /// The environment variable that would have resolved it.
    pub String,
);

fn env_var_name(reference: &str) -> String {
    format!("{ENV_PREFIX}{}", reference.to_uppercase().replace('-', "_"))
}

fn missing(reference: &str) -> CredentialError {
    CredentialError(reference.to_owned(), env_var_name(reference))
}

/// Resolves a credential from a credentials directory.
///
/// # Errors
/// Returns [`CredentialError`] naming the reference when no file provides
/// it. Callers check this before spawn, so a run never starts without its
/// credentials.
pub fn resolve_file(dir: &Path, reference: &str) -> Result<Secret, CredentialError> {
    std::fs::read_to_string(dir.join(reference))
        .map(|value| Secret::new(value.trim()))
        .map_err(|_| missing(reference))
}

/// Resolves a credential reference to its value.
///
/// # Errors
/// Returns [`CredentialError`] naming the reference when neither the
/// environment nor `$BUREAU_CREDENTIALS_DIR` provides it.
pub fn resolve(reference: &str) -> Result<Secret, CredentialError> {
    // The process environment boundary: credentials resolve from the
    // daemon's own environment.
    let env_var = std::env::var;
    let name = env_var_name(reference);
    if let Ok(value) = env_var(name.as_str()) {
        return Ok(Secret::new(value));
    }
    if let Ok(dir) = env_var(DIR_VAR) {
        return resolve_file(Path::new(&dir), reference);
    }
    Err(missing(reference))
}
