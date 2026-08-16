//! One isolated concurrent member.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::adapters::{Execution, Usage};
use crate::config::StepDef;
use crate::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};
use crate::git::Worktree;

use super::super::execute;
use super::super::gitcmd;
use super::super::machine::{RunCtx, WtCtx};

pub(super) async fn run(
    ctx: RunCtx,
    mirror: PathBuf,
    root: PathBuf,
    snapshot: String,
    step: StepDef,
) -> (String, Execution) {
    let name = step.name.clone();
    let member_cancel = cancel_path(&root, &name);
    let watcher = tokio::spawn(mirror_cancel(ctx.cancel_path(), member_cancel));
    let execution = run_inner(&ctx, &mirror, &root, &snapshot, &step).await;
    watcher.abort();
    (name, execution)
}

async fn run_inner(
    ctx: &RunCtx,
    mirror: &Path,
    root: &Path,
    snapshot: &str,
    step: &StepDef,
) -> Execution {
    let worktree = match worktree(mirror, root, snapshot, &step.name).await {
        Ok(worktree) => worktree,
        Err(error) => return failed(&format!("creating concurrent worktree failed: {error}")),
    };
    let request = execute::build_request(ctx, step, worktree.worktree.path());
    if let Some(reason) = execute::trust_check(&ctx.plan, step, &request) {
        return blocked(&reason);
    }
    execute::execute(ctx, &worktree, step, &request).await
}

async fn worktree(
    mirror: &Path,
    root: &Path,
    snapshot: &str,
    member: &str,
) -> Result<WtCtx, String> {
    let directory = member_root(root, member).join("wt");
    std::fs::create_dir_all(directory.parent().unwrap_or(root))
        .map_err(|error| error.to_string())?;
    clear_stale(mirror, &directory).await;
    let worktree = Worktree::create(mirror, &directory, member, true)
        .await
        .map_err(|error| error.to_string())?;
    gitcmd::git(&["reset", "--hard", snapshot], worktree.path(), &[]).await?;
    Ok(WtCtx {
        worktree,
        mirror: mirror.to_path_buf(),
        branch: format!("concurrent-{}", safe(member)),
        start_head: snapshot.to_owned(),
    })
}

async fn clear_stale(mirror: &Path, directory: &Path) {
    let directory = directory.to_string_lossy();
    let _ = gitcmd::git(&["worktree", "remove", "--force", &directory], mirror, &[]).await;
    let _ = gitcmd::git(&["worktree", "prune"], mirror, &[]).await;
}

pub(super) fn cancel_path(root: &Path, member: &str) -> PathBuf {
    member_root(root, member).join("CANCEL")
}

fn member_root(root: &Path, member: &str) -> PathBuf {
    root.join(safe(member))
}

pub(super) fn safe(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

async fn mirror_cancel(run_cancel: PathBuf, member_cancel: PathBuf) {
    loop {
        if run_cancel.exists() {
            let reason =
                std::fs::read_to_string(&run_cancel).unwrap_or_else(|_| "cancelled".to_owned());
            let _ = std::fs::write(member_cancel, reason);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

pub(super) fn failed(message: &str) -> Execution {
    synthetic(StepOutcome::Failure, message)
}

fn blocked(message: &str) -> Execution {
    synthetic(StepOutcome::Blocked, message)
}

fn synthetic(outcome: StepOutcome, message: &str) -> Execution {
    let result = StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    };
    Execution::new(result, Usage::zero("engine"))
}
