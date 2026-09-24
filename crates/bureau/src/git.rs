//! Layer 6: git (DESIGN.md section 7). Shells out to the `git` binary
//! through the layer-0 process contract; no `git2`/libgit2.
//!
//! - One bare mirror per remote in the checkout cache, keyed by a hash
//!   of the URL, with a `flock` lock file beside it. Every refresh, and
//!   every run-branch cut that follows one, holds that lock, so
//!   concurrent runs and processes never race on the mirror's refs.
//!   Refreshes spare branches checked out by live worktrees.
//! - One worktree per run, on a branch carrying the assignment's
//!   `branch_prefix` so cleanup is one glob.
//! - Worktree teardown is idempotent and runs on the unwind path via
//!   `Drop`, unless retained for external execution recovery.
//!
//! Credentials travel only in `http.extraheader` config for the single
//! command and in the scrub list, which holds every form they take:
//! the raw secret, the base64 `user:secret` pair argv carries, and the
//! full `AUTHORIZATION: Basic` header value. They never land in the
//! run log, the mirror's stored remote URL, or on disk. They are
//! visible in the container's process table for the duration of the
//! push; the container is the sandbox boundary (DESIGN.md section 10).

mod commit;
mod lock;
/// Committed-snapshot reads: exact-commit worktrees, ref resolution, blobs.
pub mod snapshot;
mod worktree;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::forge::ForgeKind;
use crate::process::{Secret, SpawnOutcome, SpawnRequest, SpawnResult, spawn};

pub use lock::MirrorLock;
pub use worktree::Worktree;

/// The per-command timeout for git operations.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// A git operation failed. Output shown was already secret-scrubbed by
/// the layer-0 capture boundary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The `git` process itself failed.
    #[error("git {args} failed ({outcome}): {detail}")]
    Command {
        /// The arguments passed to git.
        args: String,
        /// How the process ended.
        outcome: String,
        /// Scrubbed stderr / spawn failure detail.
        detail: String,
    },
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A resolved credential for git-over-HTTPS auth.
#[derive(Clone)]
pub struct Credential {
    user: &'static str,
    secret: Secret,
}

/// Maps a forge kind to its git-over-HTTPS credential shape.
#[must_use]
pub const fn credential_for(forge: ForgeKind, secret: Secret) -> Credential {
    let user = match forge {
        ForgeKind::Ado => "pat",
        ForgeKind::Github => "x-access-token",
    };
    Credential { user, secret }
}

/// Base64-encode without a dependency (the approved crate list has none).
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    data.chunks(3)
        .flat_map(|chunk| {
            let bits = chunk
                .iter()
                .fold(0usize, |acc, &byte| (acc << 8) | usize::from(byte))
                << (8 * (3 - chunk.len()));
            (0..4).map(move |i| {
                if i <= chunk.len() {
                    char::from(TABLE[(bits >> (18 - 6 * i)) & 63])
                } else {
                    '='
                }
            })
        })
        .collect()
}

/// The `-c http.extraheader=...` argv carrying `credential`.
///
/// Every form the credential takes joins the scrub list: the raw secret,
/// the base64 `user:secret` pair argv carries, and the full
/// `AUTHORIZATION: Basic` value a reflected error page would echo back.
#[must_use]
pub fn auth_args(credential: &Credential, secrets: &mut Vec<Secret>) -> Vec<String> {
    let user = credential.user;
    let pair = base64(format!("{user}:{}", credential.secret.expose()).as_bytes());
    secrets.push(credential.secret.clone());
    secrets.push(Secret::new(pair.as_str()));
    secrets.push(Secret::new(format!("AUTHORIZATION: Basic {pair}")));
    let header = format!("http.extraheader=AUTHORIZATION: Basic {pair}");
    vec!["-c".to_owned(), header]
}

fn check(result: SpawnResult, args: &[&str]) -> Result<Vec<u8>, Error> {
    if result.outcome == SpawnOutcome::Exited && result.exit_code == Some(0) {
        return Ok(result.stdout);
    }
    let detail = String::from_utf8_lossy(&result.stderr);
    Err(Error::Command {
        args: args.join(" "),
        outcome: format!("{:?}", result.outcome),
        detail: detail.trim().chars().take(500).collect(),
    })
}

async fn git(
    args: &[&str],
    dir: &Path,
    credential: Option<&Credential>,
    secrets: &mut Vec<Secret>,
) -> Result<Vec<u8>, Error> {
    let mut command = vec!["git".to_owned()];
    let env = BTreeMap::from([("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned())]);
    if let Some(cred) = credential {
        command.extend(auth_args(cred, secrets));
    }
    command.extend(args.iter().map(|s| (*s).to_owned()));
    let result = spawn(SpawnRequest {
        argv: command,
        dir: dir.to_path_buf(),
        env,
        stdin: Vec::new(),
        timeout: GIT_TIMEOUT,
        secrets: std::mem::take(secrets),
        log: None,
        cancel: None,
    })
    .await;
    check(result, args)
}

/// Negative refspecs for the branches the mirror's worktrees have
/// checked out. Run branches exist only here until pushed, so the
/// mirror refspec's `--prune` would delete them under live runs.
async fn live_branches(dir: &Path) -> Result<Vec<String>, Error> {
    let args = ["worktree", "list", "--porcelain"];
    let listed = git(&args, dir, None, &mut Vec::new()).await?;
    let listed = String::from_utf8_lossy(&listed);
    let branches = listed
        .lines()
        .filter_map(|line| line.strip_prefix("branch "));
    Ok(branches.map(|branch| format!("^{branch}")).collect())
}

/// `git fetch --prune` with the mirror refspec, sparing live branches.
async fn fetch(dir: &Path, credential: Option<&Credential>) -> Result<(), Error> {
    let mut args = ["fetch", "--prune", "origin", "+refs/*:refs/*"]
        .map(str::to_owned)
        .to_vec();
    args.extend(live_branches(dir).await?);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    git(&args, dir, credential, &mut Vec::new()).await?;
    Ok(())
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

    /// The lock file serializing mutation of `url`'s mirror; it sits next
    /// to the mirror directory so clearing the cache removes both.
    fn lock_path(&self, url: &str) -> PathBuf {
        self.mirror_dir(url).with_extension("lock")
    }

    /// Ensures an up-to-date bare mirror of `url` exists and returns its
    /// path: `git clone --mirror` on first use, `git fetch --prune` after.
    /// Waits up to [`GIT_TIMEOUT`] for other writers of the mirror.
    ///
    /// # Errors
    /// Propagates lock, git, and filesystem failures.
    pub async fn mirror(
        &self,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<PathBuf, Error> {
        let (dir, _lock) = self.mirror_locked(url, credential, GIT_TIMEOUT).await?;
        Ok(dir)
    }

    /// [`Self::mirror`], returning the mirror's lock still held. Hold it
    /// across every change to the mirror's refs and worktree registrations
    /// (such as cutting a run branch) that must not interleave with
    /// another run's refresh. Waits at most `wait` for the lock.
    ///
    /// # Errors
    /// Propagates lock, git, and filesystem failures; a lock still busy
    /// after `wait` is an [`std::io::ErrorKind::TimedOut`] error.
    pub async fn mirror_locked(
        &self,
        url: &str,
        credential: Option<&Credential>,
        wait: Duration,
    ) -> Result<(PathBuf, MirrorLock), Error> {
        let lock = lock::acquire(self.lock_path(url), wait).await?;
        let dir = self.mirror_dir(url);
        self.refresh(url, &dir, credential).await?;
        Ok((dir, lock))
    }

    /// `git clone --mirror` on first use, `git fetch --prune` after; the
    /// caller holds the mirror's lock, so the existence check cannot race.
    async fn refresh(
        &self,
        url: &str,
        dir: &Path,
        credential: Option<&Credential>,
    ) -> Result<(), Error> {
        if dir.exists() {
            fetch(dir, credential).await
        } else {
            self.clone_mirror(url, dir, credential).await
        }
    }

    async fn clone_mirror(
        &self,
        url: &str,
        dir: &Path,
        credential: Option<&Credential>,
    ) -> Result<(), Error> {
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut secrets = Vec::new();
        let args = ["clone", "--mirror", url, &name];
        git(&args, &self.root, credential, &mut secrets).await?;
        Ok(())
    }
}
