//! Layer 6: git (DESIGN.md section 7). Shells out to the `git` binary
//! through the layer-0 process contract; no `git2`/libgit2.
//!
//! - One bare mirror per remote in the checkout cache, keyed by a hash
//!   of the URL.
//! - One worktree per run, on a branch carrying the assignment's
//!   `branch_prefix` so cleanup is one glob.
//! - Worktree teardown is idempotent and runs on the unwind path via
//!   `Drop`, not only the happy path.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::process::Secret;

/// A git operation failed. Output shown was already secret-scrubbed by
/// the layer-0 capture boundary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The `git` process itself failed.
    #[error("git {args} failed: {detail}")]
    Command {
        /// The arguments passed to git.
        args: String,
        /// Scrubbed stderr / spawn failure detail.
        detail: String,
    },
    /// A path the operation needed did not exist.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Bare-mirror cache, one directory per remote URL.
#[derive(Debug, Clone)]
pub struct CheckoutCache {
    root: PathBuf,
}

impl CheckoutCache {
    /// A cache rooted at `root` (created lazily).
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The cache directory for a URL: a hash of the URL, so remotes with
    /// awkward names still map to one stable path.
    #[must_use]
    pub fn mirror_dir(&self, url: &str) -> PathBuf {
        use std::hash::{Hash, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        self.root.join(format!("{:016x}", hasher.finish()))
    }

    /// Ensures an up-to-date bare mirror of `url` exists and returns its
    /// path: `git clone --mirror` on first use, `git fetch --prune`
    /// after. `credential` (when the repo is not public) is injected via
    /// the child environment only — never into argv or the URL on disk.
    ///
    /// # Errors
    /// Propagates git failures.
    pub async fn mirror(&self, url: &str, credential: Option<&Secret>) -> Result<PathBuf, Error> {
        let _ = (url, credential);
        tokio::task::yield_now().await;
        todo!(
            "clone --mirror or fetch --prune through process::spawn; env carries GIT_ASKPASS-free auth; secrets list carries the credential"
        )
    }
}

/// One run's worktree. Teardown runs on drop: `git worktree remove
/// --force`, blocking briefly and synchronously because `Drop` cannot be
/// async — that is the point of the guard (DESIGN.md layer 6).
#[derive(Debug)]
pub struct Worktree {
    mirror: PathBuf,
    dir: PathBuf,
    branch: String,
}

impl Worktree {
    /// `git worktree add --no-track -b <branch> <dir>` off the mirror, or
    /// with `--detach` when `detach` (read-only steps: git refuses the
    /// same branch in two worktrees).
    ///
    /// # Errors
    /// Propagates git failures.
    pub async fn create(
        mirror: &Path,
        dir: &Path,
        branch: &str,
        detach: bool,
    ) -> Result<Self, Error> {
        let _ = (mirror, dir, branch, detach);
        tokio::task::yield_now().await;
        todo!("git worktree add through process::spawn")
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

    /// Pushes the branch to `remote_url`, returning scrubbed output.
    ///
    /// # Errors
    /// Propagates git failures.
    pub async fn push(&self, remote_url: &str, credential: Option<&Secret>) -> Result<(), Error> {
        let _ = (remote_url, credential);
        tokio::task::yield_now().await;
        todo!("git push through process::spawn with credential env")
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        let _ = (&self.mirror, &self.dir);
        todo!(
            "git worktree remove --force via std::process (sync; Drop cannot be async); ignore already-gone errors so teardown is idempotent"
        )
    }
}

/// The default per-command timeout for git operations.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(300);
