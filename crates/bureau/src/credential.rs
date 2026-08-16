//! Secret-free settings-aware credential resolution.

use std::path::Path;

use crate::process::Secret;
use crate::setup::{CredentialSource, Settings};

/// Resolves one credential declared in local settings.
///
/// # Errors
/// Rejects undeclared, missing, empty, symlinked, or unreadable sources.
pub fn resolve(settings: &Settings, reference: &str) -> Result<Secret, Error> {
    let source = settings
        .credentials
        .get(reference)
        .ok_or_else(|| Error::Undeclared(reference.to_owned()))?;
    match source {
        CredentialSource::Environment { variable } => environment(variable, reference),
        CredentialSource::Directory { path } => file(&path.join(reference), reference),
        CredentialSource::File { path } => file(path, reference),
    }
}

fn environment(variable: &str, reference: &str) -> Result<Secret, Error> {
    let value = std::env::var(variable).map_err(|_| Error::Unavailable(reference.to_owned()))?;
    nonempty(value, reference)
}

fn file(path: &Path, reference: &str) -> Result<Secret, Error> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| Error::Unavailable(reference.to_owned()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::Unsafe(reference.to_owned()));
    }
    let value =
        std::fs::read_to_string(path).map_err(|_| Error::Unavailable(reference.to_owned()))?;
    nonempty(value.trim().to_owned(), reference)
}

fn nonempty(value: String, reference: &str) -> Result<Secret, Error> {
    if value.is_empty() {
        Err(Error::Unavailable(reference.to_owned()))
    } else {
        Ok(Secret::new(value))
    }
}

/// Declared credential resolution failure that never contains a secret.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reference is absent from settings.
    #[error("credential `{0}` is not declared in settings.yaml")]
    Undeclared(String),
    /// Declared source cannot provide a value.
    #[error("credential `{0}` is unavailable from its declared source")]
    Unavailable(String),
    /// Declared source is not a safe regular file.
    #[error("credential `{0}` source is not a safe regular file")]
    Unsafe(String),
}
