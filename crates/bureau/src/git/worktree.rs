//! Worktree ownership, including explicit retention for external journals.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Credential, Error, git};

fn creation_args<'a>(directory: &'a str, branch: &'a str, detach: bool) -> Vec<&'a str> {
    let mut args = vec!["worktree", "add"];
    if detach {
        args.push("--detach");
    } else {
        args.extend(["--no-track", "-b", branch]);
    }
    args.push(directory);
    args
}

fn check_identity(
    mirror: &Path,
    directory: &Path,
    branch: &str,
    output: &[u8],
) -> Result<(), Error> {
    let actual = std::str::from_utf8(output).map_err(std::io::Error::other)?;
    let mirror = std::fs::canonicalize(mirror)?;
    let directory = std::fs::canonicalize(directory)?;
    let expected = [
        mirror.to_string_lossy(),
        directory.to_string_lossy(),
        std::borrow::Cow::Borrowed(branch),
    ];
    if actual.lines().eq(expected.iter().map(AsRef::as_ref)) {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "retained worktree no longer has its recorded repository, directory, and branch",
    )
    .into())
}

async fn verify_existing(mirror: &Path, directory: &Path, branch: &str) -> Result<(), Error> {
    let args = [
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
        "--show-toplevel",
        "--abbrev-ref",
        "HEAD",
    ];
    let output = git(&args, directory, None, &mut Vec::new()).await?;
    check_identity(mirror, directory, branch, &output)
}

/// One run's worktree, normally removed on drop.
///
/// Retention is explicit and survives cancellation of the owning future.
/// A retained worktree must not be reset while an external journal depends on it.
#[derive(Debug)]
pub struct Worktree {
    mirror: PathBuf,
    dir: PathBuf,
    branch: String,
    retained: AtomicBool,
}

impl Worktree {
    fn new(mirror: &Path, dir: PathBuf, branch: &str, retained: bool) -> Self {
        Self {
            mirror: mirror.to_path_buf(),
            dir,
            branch: branch.to_owned(),
            retained: AtomicBool::new(retained),
        }
    }

    /// Creates a new run branch, or a detached read-only worktree.
    /// Relative directories are resolved against the daemon's cwd, not the mirror.
    ///
    /// # Errors
    /// Propagates git and path-resolution failures.
    pub async fn create(
        mirror: &Path,
        dir: &Path,
        branch: &str,
        detach: bool,
    ) -> Result<Self, Error> {
        let dir = std::path::absolute(dir)?;
        let directory = dir.to_string_lossy();
        let args = creation_args(&directory, branch, detach);
        git(&args, mirror, None, &mut Vec::new()).await?;
        Ok(Self::new(mirror, dir, branch, false))
    }

    /// Reopens the exact existing worktree without fetching, resetting, or removing it.
    /// Cleanup remains disabled until the external execution has been reconciled.
    ///
    /// # Errors
    /// Rejects a missing worktree or a changed repository, directory, or branch.
    pub async fn resume(mirror: &Path, dir: &Path, branch: &str) -> Result<Self, Error> {
        let dir = std::path::absolute(dir)?;
        verify_existing(mirror, &dir, branch).await?;
        Ok(Self::new(mirror, dir, branch, true))
    }

    /// Keeps files and the git registration when the guard is dropped.
    pub fn retain(&self) {
        self.retained.store(true, Ordering::Release);
    }

    /// Re-enables ordinary cleanup after external execution and descendant exit are verified.
    pub fn allow_cleanup(&self) {
        self.retained.store(false, Ordering::Release);
    }

    /// Whether dropping this guard preserves the worktree.
    #[must_use]
    pub fn is_retained(&self) -> bool {
        self.retained.load(Ordering::Acquire)
    }

    /// The run's working directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// The run branch created for this worktree.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Pushes the branch to `remote_url`.
    ///
    /// # Errors
    /// Propagates git failures.
    pub async fn push(
        &self,
        remote_url: &str,
        credential: Option<&Credential>,
    ) -> Result<(), Error> {
        let args = ["push", remote_url, &self.branch];
        git(&args, &self.dir, credential, &mut Vec::new()).await?;
        Ok(())
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        if self.is_retained() {
            return;
        }
        let removed = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.dir)
            .current_dir(&self.mirror)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .is_ok_and(|output| output.status.success());
        if !removed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}
