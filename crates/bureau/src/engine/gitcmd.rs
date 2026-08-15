//! The `git` invocations the engine owns: local inspection and
//! committing inside the worktree. Credentials never pass through here
//! — push auth goes through [`crate::git::Worktree::push`].

use std::collections::BTreeMap;
use std::path::Path;

use crate::process::{SpawnOutcome, SpawnRequest, SpawnResult, spawn};

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
    clock: fn() -> u64,
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
        clock,
        log: None,
    })
    .await;
    checked(args, &result)
}
