//! Layer 6: git (DESIGN.md section 7). Shells out to the `git` binary
//! through the layer-0 process contract; no `git2`/libgit2.
//!
//! - One bare mirror per remote in the checkout cache, keyed by a hash of the URL.
//! - One worktree per run, on a branch carrying the assignment's
//!   `branch_prefix` so cleanup is one glob.
//! - Worktree teardown is idempotent and runs on unwind via `Drop`, not only the happy path.
//!
//! Credentials travel only in `http.extraheader` config for the single
//! command and in the scrub list, which holds every form they take (see
//! `auth_args`). They never land in the run log, the mirror's stored
//! remote URL, or on disk; they sit in the process table only for the
//! push — the container is the sandbox boundary (DESIGN.md section 10).

mod auth;
mod cache;
mod worktree;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub use auth::{Credential, auth_args, credential_for};
pub use cache::CheckoutCache;
pub use worktree::Worktree;

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
    clock: fn() -> u64,
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
        clock,
        log: None,
    })
    .await;
    check(result, args)
}
