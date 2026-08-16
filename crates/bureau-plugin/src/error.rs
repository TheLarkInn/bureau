//! Plugin activation errors.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// A plugin resolution, snapshot, or temporary activation failure.
#[derive(Debug)]
pub enum Error {
    /// The role agent reference was not a `/plugin:agent` reference.
    InvalidReference(String),
    /// No approved source contained the requested plugin.
    MissingPlugin(String),
    /// Plugin or activation data was invalid.
    InvalidData {
        /// File or directory associated with the invalid data.
        path: PathBuf,
        /// Explanation of the invalid data.
        message: String,
    },
    /// A filesystem operation failed.
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying filesystem error.
        source: io::Error,
    },
    /// Activation files changed after they were injected.
    Conflict {
        /// Paths whose bytes no longer matched the injected bytes.
        paths: Vec<PathBuf>,
        /// Restoration failures, if restoring also encountered errors.
        restore_failures: Vec<String>,
    },
    /// Exact restoration failed without an activation conflict.
    Restore(Vec<String>),
}

impl Error {
    pub(crate) fn invalid(path: &Path, message: impl fmt::Display) -> Self {
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

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReference(value) => invalid_reference(formatter, value),
            Self::MissingPlugin(name) => missing_plugin(formatter, name),
            Self::InvalidData { path, message } => {
                write!(formatter, "{}: {message}", path.display())
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{operation} {}: {source}", path.display()),
            Self::Conflict {
                paths,
                restore_failures,
            } => conflict(formatter, paths, restore_failures),
            Self::Restore(failures) => restore(formatter, failures),
        }
    }
}

fn invalid_reference(formatter: &mut fmt::Formatter<'_>, value: &str) -> fmt::Result {
    write!(
        formatter,
        "use a `/plugin:agent` role agent reference; received `{value}`"
    )
}

fn missing_plugin(formatter: &mut fmt::Formatter<'_>, name: &str) -> fmt::Result {
    write!(
        formatter,
        "run `bureau setup` or install plugin `{name}` with Copilot, then retry; no enabled local, user-global, or development source was found"
    )
}

fn conflict(
    formatter: &mut fmt::Formatter<'_>,
    paths: &[PathBuf],
    failures: &[String],
) -> fmt::Result {
    if failures.is_empty() {
        return write!(
            formatter,
            "temporary plugin activation changed at {}; originals were restored and the run must escalate",
            display_paths(paths)
        );
    }
    write!(
        formatter,
        "temporary plugin activation changed at {}; restoration was incomplete: {}",
        display_paths(paths),
        failures.join("; ")
    )
}

fn restore(formatter: &mut fmt::Formatter<'_>, failures: &[String]) -> fmt::Result {
    write!(
        formatter,
        "temporary plugin restoration failed: {}",
        failures.join("; ")
    )
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
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
