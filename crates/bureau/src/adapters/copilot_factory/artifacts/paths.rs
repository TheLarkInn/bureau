use std::fs;
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use bureau_plugin::TreeSnapshot;
use serde::{Deserialize, Serialize};

use super::super::SetupError;

fn safe_session(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn check_root(root: &Path, worktree: &Path, session_id: &str) -> Result<(), String> {
    if !safe_session(session_id) {
        return Err("factory runtime session ID is not a safe directory identifier".to_owned());
    }
    TreeSnapshot::require_outside(root, worktree).map_err(|error| error.to_string())
}

fn check_private(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(
            "factory state requires a private plain directory on a native filesystem".into(),
        );
    }
    Ok(())
}

fn private_directory(path: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|error| format!("creating private factory directory: {error}"))?;
    check_private(path)
}

/// Runtime-owned mutable storage, separate from immutable executable pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Storage {
    /// Runtime-owned configuration, database, journal, and results.
    pub copilot_home: PathBuf,
    /// Empty private HOME rather than the user's home.
    pub user_home: PathBuf,
    /// Runtime-owned session directory.
    pub session: PathBuf,
}

impl Storage {
    fn new(root: &Path, session_id: &str) -> Self {
        let copilot_home = root.join("copilot");
        Self {
            session: copilot_home.join("session-state").join(session_id),
            copilot_home,
            user_home: root.join("home"),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePaths {
    root: PathBuf,
    runtime: PathBuf,
    copilot_home: PathBuf,
    user_home: PathBuf,
    session: PathBuf,
    provider: PathBuf,
}

impl From<Paths> for WirePaths {
    fn from(paths: Paths) -> Self {
        Self {
            root: paths.root,
            runtime: paths.runtime,
            copilot_home: paths.storage.copilot_home,
            user_home: paths.storage.user_home,
            session: paths.storage.session,
            provider: paths.provider,
        }
    }
}

/// Deterministic paths from the durable, caller-selected runtime session ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "WirePaths", into = "WirePaths")]
pub struct Paths {
    /// One invocation's private root.
    pub root: PathBuf,
    /// Pinned runtime and SDK code.
    pub runtime: PathBuf,
    /// Actual session-scoped extension code.
    pub provider: PathBuf,
    /// Mutable runtime state, preserved across reconnects.
    pub storage: Storage,
}

impl From<WirePaths> for Paths {
    fn from(paths: WirePaths) -> Self {
        Self {
            root: paths.root,
            runtime: paths.runtime,
            provider: paths.provider,
            storage: Storage {
                copilot_home: paths.copilot_home,
                user_home: paths.user_home,
                session: paths.session,
            },
        }
    }
}

impl Paths {
    fn new_inner(root: &Path, worktree: &Path, session_id: &str) -> Result<Self, String> {
        check_root(root, worktree, session_id)?;
        let root = std::path::absolute(root).map_err(|error| error.to_string())?;
        let storage = Storage::new(&root, session_id);
        Ok(Self {
            runtime: root.join("runtime"),
            provider: storage.session.join("extensions").join(session_id),
            root,
            storage,
        })
    }

    /// Builds paths without creating files or starting a runtime.
    ///
    /// # Errors
    /// Rejects unsafe IDs and roots within the agent worktree.
    pub fn new(root: &Path, worktree: &Path, session_id: &str) -> Result<Self, SetupError> {
        Self::new_inner(root, worktree, session_id).map_err(SetupError::Material)
    }

    pub(super) fn create_private(&self) -> Result<(), String> {
        for directory in [
            &self.root,
            &self.storage.copilot_home,
            &self.storage.user_home,
        ] {
            private_directory(directory)?;
        }
        Ok(())
    }

    pub(super) fn verify_private(&self) -> Result<(), String> {
        for directory in [
            &self.root,
            &self.storage.copilot_home,
            &self.storage.user_home,
        ] {
            check_private(directory)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
