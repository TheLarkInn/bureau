use std::io;
use std::path::{Path, PathBuf};

use crate::contract::StepRequest;

use super::{BUREAU_STEP_REQUEST, BUREAU_STEP_RESULT};

/// Files used by one MCP step session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    request: PathBuf,
    result: PathBuf,
}

impl Paths {
    /// Creates a path pair. [`serve`](super::serve) validates it before
    /// reading any protocol messages.
    #[must_use]
    pub fn new(request: impl Into<PathBuf>, result: impl Into<PathBuf>) -> Self {
        Self {
            request: request.into(),
            result: result.into(),
        }
    }

    /// Reads the path pair from the MCP environment.
    ///
    /// # Errors
    /// Returns an error when either variable is missing or empty.
    pub fn from_env() -> io::Result<Self> {
        Ok(Self::new(
            env_path(BUREAU_STEP_REQUEST)?,
            env_path(BUREAU_STEP_RESULT)?,
        ))
    }

    /// Immutable request file.
    #[must_use]
    pub fn request(&self) -> &Path {
        &self.request
    }

    /// One-shot result file.
    #[must_use]
    pub fn result(&self) -> &Path {
        &self.result
    }

    pub(super) fn validate(&self) -> io::Result<StepRequest> {
        validate_absolute(self)?;
        let request = load_request(&self.request)?;
        let directory = validate_result_path(self)?;
        validate_outside_worktree(&directory, &request.worktree)?;
        Ok(request)
    }
}

fn env_path(name: &str) -> io::Result<PathBuf> {
    let value = std::env::var_os(name).ok_or_else(|| missing_env(name))?;
    if value.is_empty() {
        return Err(missing_env(name));
    }
    Ok(PathBuf::from(value))
}

fn missing_env(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("required environment variable {name} is missing"),
    )
}

fn validate_absolute(paths: &Paths) -> io::Result<()> {
    if paths.request.is_absolute() && paths.result.is_absolute() {
        return Ok(());
    }
    Err(invalid_path(
        "MCP request and result paths must be absolute",
    ))
}

fn load_request(path: &Path) -> io::Result<StepRequest> {
    let bytes = std::fs::read(path)?;
    StepRequest::from_json(&bytes).map_err(invalid_data)
}

fn validate_result_path(paths: &Paths) -> io::Result<PathBuf> {
    let request_dir = std::fs::canonicalize(parent(&paths.request)?)?;
    let result_dir = std::fs::canonicalize(parent(&paths.result)?)?;
    if request_dir != result_dir {
        return Err(invalid_path(
            "MCP request and result must share a directory",
        ));
    }
    require_absent(&paths.result)?;
    Ok(request_dir)
}

fn parent(path: &Path) -> io::Result<&Path> {
    path.parent()
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or_else(|| invalid_path("MCP path has no parent directory"))
}

fn require_absent(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "MCP result path already exists",
        )),
    }
}

fn validate_outside_worktree(directory: &Path, worktree: &Path) -> io::Result<()> {
    let worktree = std::fs::canonicalize(worktree)?;
    if directory.starts_with(worktree) {
        return Err(invalid_path("MCP session directory is inside the worktree"));
    }
    Ok(())
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_path(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
