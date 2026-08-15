//! Layer 6: git (DESIGN.md section 7). Shells out to the `git` binary
//! through the layer-0 process contract; no `git2`/libgit2.
//!
//! - One bare mirror per remote in the checkout cache, keyed by a hash
//!   of the URL.
//! - One worktree per run, on a branch carrying the assignment's
//!   `branch_prefix` so cleanup is one glob.
//! - Worktree teardown is idempotent and runs on the unwind path via
//!   `Drop`, not only the happy path.
//!
//! Credentials travel only in `http.extraheader` config for the single
//! command and in the scrub list, which holds every form they take:
//! the raw secret, the base64 `user:secret` pair argv carries, and the
//! full `AUTHORIZATION: Basic` header value. They never land in the
//! run log, the mirror's stored remote URL, or on disk. They are
//! visible in the container's process table for the duration of the
//! push; the container is the sandbox boundary (DESIGN.md section 10).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::ForgeKind;
use crate::process::{Secret, SpawnOutcome, SpawnRequest, SpawnResult, spawn};

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
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .fold(0usize, |acc, &b| (acc << 8) | usize::from(b))
            << (8 * (3 - chunk.len()));
        for i in 0..4 {
            let keep = chunk.len() + 1;
            out.push(if i < keep {
                char::from(TABLE[(n >> (18 - 6 * i)) & 63])
            } else {
                '='
            });
        }
    }
    out
}

/// The `-c http.extraheader=...` argv carrying `credential`.
///
/// Every form the credential takes joins the scrub list: the raw
/// secret, the base64 `user:secret` pair that sits in argv and on the
/// wire, and the full `AUTHORIZATION: Basic` value a reflected error
/// page would echo back.
#[must_use]
pub fn auth_args(credential: &Credential, secrets: &mut Vec<Secret>) -> Vec<String> {
    let user = credential.user;
    let pair = base64(format!("{user}:{}", credential.secret.expose()).as_bytes());
    secrets.push(credential.secret.clone());
    secrets.push(Secret::new(pair.as_str()));
    secrets.push(Secret::new(format!("AUTHORIZATION: Basic {pair}")));
    vec![
        "-c".to_owned(),
        format!("http.extraheader=AUTHORIZATION: Basic {pair}"),
    ]
}

fn check(result: SpawnResult, args: &[&str]) -> Result<Vec<u8>, Error> {
    if result.outcome == SpawnOutcome::Exited && result.exit_code == Some(0) {
        return Ok(result.stdout);
    }
    Err(Error::Command {
        args: args.join(" "),
        outcome: format!("{:?}", result.outcome),
        detail: String::from_utf8_lossy(&result.stderr)
            .trim()
            .chars()
            .take(500)
            .collect(),
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
    })
    .await;
    check(result, args)
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
    /// after.
    ///
    /// # Errors
    /// Propagates git and filesystem failures.
    pub async fn mirror(
        &self,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<PathBuf, Error> {
        let dir = self.mirror_dir(url);
        let mut secrets = Vec::new();
        if dir.exists() {
            git(&["fetch", "--prune"], &dir, credential, &mut secrets).await?;
        } else {
            std::fs::create_dir_all(&self.root)?;
            let name = dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            git(
                &["clone", "--mirror", url, &name],
                &self.root,
                credential,
                &mut secrets,
            )
            .await?;
        }
        Ok(dir)
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
    /// same branch in two worktrees). `--no-track` requires a new branch,
    /// so it is omitted in detach mode. A relative `dir` is resolved
    /// against the daemon's cwd — never the mirror, which is the child
    /// process's cwd and would swallow the worktree into the cache.
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
        let dir_arg = dir.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "add"];
        if detach {
            args.push("--detach");
        } else {
            args.extend(["--no-track", "-b", branch]);
        }
        args.push(&dir_arg);
        let mut secrets = Vec::new();
        git(&args, mirror, None, &mut secrets).await?;
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
    ) -> Result<(), Error> {
        let mut secrets = Vec::new();
        git(
            &["push", remote_url, &self.branch],
            &self.dir,
            credential,
            &mut secrets,
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
            .args(["-c", "safe.bareRepository=all"])
            .args(["worktree", "remove", "--force"])
            .arg(&self.dir)
            .current_dir(&self.mirror)
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .is_ok_and(|o| o.status.success());
        if !removed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}
