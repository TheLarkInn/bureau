//! One run's worktree guard: creation, push, and teardown on drop.

use std::path::{Path, PathBuf};

use super::{Credential, Error, git};

/// One run's worktree. Teardown runs on drop: `git worktree remove
/// --force`, blocking briefly and synchronously because `Drop` cannot
/// be async — that is the point of the guard (DESIGN.md layer 6).
#[derive(Debug)]
pub struct Worktree {
    mirror: PathBuf,
    dir: PathBuf,
    branch: String,
}

impl Worktree {
    /// `git worktree add --no-track -b <branch> <dir>` off the mirror,
    /// or `--detach` when `detach` (read-only steps: git refuses the
    /// same branch in two worktrees; `--no-track` requires a new
    /// branch, so it is omitted in detach mode). A relative `dir` is
    /// resolved against the daemon's cwd — never the mirror, the child
    /// process's cwd, which would swallow the worktree into the cache.
    ///
    /// # Errors
    /// Propagates git and path-resolution failures.
    pub async fn create(
        mirror: &Path,
        dir: &Path,
        branch: &str,
        detach: bool,
        clock: fn() -> u64,
    ) -> Result<Self, Error> {
        let dir = std::path::absolute(dir)?;
        let dir_arg = dir.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "add"];
        if detach {
            args.push("--detach");
        } else {
            args.extend(["--no-track", "-b", branch]);
        }
        args.push(&dir_arg);
        let mut secrets = Vec::new();
        git(&args, mirror, None, &mut secrets, clock).await?;
        Ok(Self {
            mirror: mirror.to_path_buf(),
            dir,
            branch: branch.to_owned(),
        })
    }

    /// The worktree path — the only directory a run's steps may write to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// The run branch created for this worktree.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Pushes the branch to `remote_url` (`git push <url> <branch>`).
    ///
    /// # Errors
    /// Propagates git failures.
    pub async fn push(
        &self,
        remote_url: &str,
        credential: Option<&Credential>,
        clock: fn() -> u64,
    ) -> Result<(), Error> {
        let mut secrets = Vec::new();
        git(
            &["push", remote_url, &self.branch],
            &self.dir,
            credential,
            &mut secrets,
            clock,
        )
        .await?;
        Ok(())
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        // Sync std::process on purpose: Drop cannot be async. Idempotent:
        // an already-removed worktree or a missing mirror both fall
        // through to the directory sweep.
        let removed = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.dir)
            .current_dir(&self.mirror)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .is_ok_and(|o| o.status.success());
        if !removed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}
