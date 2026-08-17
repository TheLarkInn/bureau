//! Plugin activation errors.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// A plugin resolution, snapshot, or temporary activation failure.
#[derive(Debug, Error)]
pub enum Error {
    /// The role agent reference was not a `/plugin:agent` reference.
    #[error("use a `/plugin:agent` role agent reference; received `{0}`")]
    InvalidReference(String),
    /// No approved source contained the requested plugin.
    #[error(
        "run `bureau setup` or install plugin `{0}` with Copilot, then retry; no enabled local, user-global, or development source was found"
    )]
    MissingPlugin(String),
    /// Plugin or activation data was invalid.
    #[error("{}: {message}", .path.display())]
    InvalidData {
        /// File or directory associated with the invalid data.
        path: PathBuf,
        /// Explanation of the invalid data.
        message: String,
    },
    /// A filesystem operation failed.
    #[error("{operation} {}", .path.display())]
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying filesystem error.
        source: io::Error,
    },
    /// Activation files changed after they were injected.
    #[error("{}", conflict_message(.paths, .restore_failures))]
    Conflict {
        /// Paths whose bytes no longer matched the injected bytes.
        paths: Vec<PathBuf>,
        /// Restoration failures, if restoring also encountered errors.
        restore_failures: Vec<String>,
    },
    /// Exact restoration failed without an activation conflict.
    #[error("temporary plugin restoration failed: {}", .0.join("; "))]
    Restore(Vec<String>),
    /// Copilot plugin installation failed.
    #[error("Copilot plugin installation failed: {0}")]
    Install(String),
}

impl Error {
    pub(crate) fn invalid(path: &Path, message: impl std::fmt::Display) -> Self {
        Self::InvalidData {
            path: path.to_path_buf(),
            message: message.to_string(),
        }
    }

    pub(crate) fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn conflict_message(paths: &[PathBuf], failures: &[String]) -> String {
    let changed = display_paths(paths);
    if failures.is_empty() {
        return format!(
            "temporary plugin activation changed at {changed}; originals were restored and the run must escalate"
        );
    }
    format!(
        "temporary plugin activation changed at {changed}; restoration was incomplete: {}",
        failures.join("; ")
    )
}
