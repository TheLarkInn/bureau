//! Durable Git checkpoint after each completed step.

use super::context::{self, RunCtx, WtCtx};
use super::gitcmd;
use crate::adapters::Execution;
use crate::config::StepDef;
use crate::contract::StepOutcome;
use crate::runlog::{self, EventKind};

async fn commit(wt: &WtCtx, step: &str) -> Result<(), String> {
    gitcmd::git(&["add", "-A"], wt.worktree.path(), &[]).await?;
    let message = format!("bureau checkpoint: {step}");
    gitcmd::git(
        &["commit", "-m", &message],
        wt.worktree.path(),
        &gitcmd::IDENTITY,
    )
    .await?;
    Ok(())
}

async fn commit_if_changed(wt: &WtCtx, step: &str) -> Result<(), String> {
    let status = gitcmd::git(&["status", "--porcelain"], wt.worktree.path(), &[]).await?;
    if !status.is_empty() {
        commit(wt, step).await?;
    }
    Ok(())
}

pub(super) async fn save(wt: &WtCtx, step: &str) -> Result<String, String> {
    commit_if_changed(wt, step).await?;
    gitcmd::git(&["rev-parse", "HEAD"], wt.worktree.path(), &[]).await
}

fn record(ctx: &mut RunCtx, wt: &WtCtx, step: &StepDef, commit: &str) {
    ctx.base_commit.get_or_insert_with(|| wt.start_head.clone());
    ctx.checkpoint = Some(commit.to_owned());
    let data = runlog::checkpoint(&step.name, &wt.start_head, commit);
    context::append(ctx, EventKind::Checkpoint, data);
}

pub(super) async fn save_result(
    ctx: &mut RunCtx,
    wt: &WtCtx,
    step: &StepDef,
    result: &mut Execution,
) {
    match save(wt, &step.name).await {
        Ok(commit) => record(ctx, wt, step, &commit),
        Err(error) => {
            result.result.outcome = StepOutcome::Failure;
            result.result.message = format!("checkpointing step `{}` failed: {error}", step.name);
        }
    }
}
