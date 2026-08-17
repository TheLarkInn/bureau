//! The `git` invocations the engine owns: local inspection and
//! committing inside the worktree. Credentials never pass through here
//! — push auth goes through [`crate::git::Worktree::push`].

use std::collections::BTreeMap;
use std::path::Path;

use crate::process::{SpawnOutcome, SpawnRequest, SpawnResult, spawn};

/// Commit identity inside worktrees; no host Git config is consulted.
pub(super) const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "bureau"),
    ("GIT_AUTHOR_EMAIL", "bureau@localhost"),
    ("GIT_COMMITTER_NAME", "bureau"),
    ("GIT_COMMITTER_EMAIL", "bureau@localhost"),
];

/// Fails unless git exited 0; returns trimmed stdout.
fn checked(args: &[&str], result: &SpawnResult) -> Result<String, String> {
    if result.outcome == SpawnOutcome::Exited && result.exit_code == Some(0) {
        return Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned());
    }
    let detail = String::from_utf8_lossy(&result.stderr);
    Err(format!("git {} failed: {}", args.join(" "), detail.trim()))
}

/// Runs `git args` in `dir` through the layer-0 contract and returns
/// trimmed stdout. `extra_env` carries the commit identity.
///
/// # Errors
/// Returns the scrubbed stderr tail unless git exited 0.
pub(super) async fn git(
    args: &[&str],
    dir: &Path,
    extra_env: &[(&str, &str)],
) -> Result<String, String> {
    let mut env = BTreeMap::from([("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned())]);
    env.extend(
        extra_env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    let command = std::iter::once("git".to_owned())
        .chain(args.iter().map(|s| (*s).to_owned()))
        .collect();
    let result = spawn(SpawnRequest {
        argv: command,
        dir: dir.to_path_buf(),
        env,
        stdin: Vec::new(),
        timeout: crate::git::GIT_TIMEOUT,
        secrets: Vec::new(),
        log: None,
        cancel: None,
    })
    .await;
    checked(args, &result)
}
