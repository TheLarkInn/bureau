//! The `done` terminal: commit, push, open the PR (DESIGN.md section 11
//! step 9). A run that changed nothing exits `NoWork` instead.

use std::path::Path;

use super::machine::{RunCtx, WtCtx, primary_repo};
use super::{gitcmd, settle};
use crate::contract::StepOutcome;
use crate::forge::{Pr, PrRequest};
use crate::git::credential_for;

/// Commit identity inside worktrees; no host git config is consulted.
const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "bureau"),
    ("GIT_AUTHOR_EMAIL", "bureau@localhost"),
    ("GIT_COMMITTER_NAME", "bureau"),
    ("GIT_COMMITTER_EMAIL", "bureau@localhost"),
];

/// Runs the `done` terminal: change detection, commit, push, PR.
pub(super) async fn finalize(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    match changed(wt).await {
        Ok(false) => (
            StepOutcome::NoWork,
            "run produced no changes".to_owned(),
            None,
        ),
        Ok(true) => land(ctx, wt).await,
        Err(message) => (StepOutcome::Failure, message, None),
    }
}

/// Whether the worktree moved past its start commit or holds edits.
async fn changed(wt: &WtCtx) -> Result<bool, String> {
    let head = gitcmd::git(&["rev-parse", "HEAD"], wt.worktree.path(), &[]).await?;
    if head != wt.start_head {
        return Ok(true);
    }
    let status = gitcmd::git(&["status", "--porcelain"], wt.worktree.path(), &[]).await?;
    Ok(!status.is_empty())
}

/// Commits leftover edits, pushes the branch, and opens the PR.
async fn land(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    let message = format!("bureau: {} ({})", ctx.plan.item.title, ctx.plan.run_id);
    if let Err(error) = commit_all(wt, &message).await {
        return (StepOutcome::Failure, error, None);
    }
    push_pr(ctx, wt).await
}

/// `git add -A && git commit` for changes steps left uncommitted.
async fn commit_all(wt: &WtCtx, message: &str) -> Result<(), String> {
    gitcmd::git(&["add", "-A"], wt.worktree.path(), &[]).await?;
    gitcmd::git(&["commit", "-m", message], wt.worktree.path(), &IDENTITY).await?;
    Ok(())
}

/// Pushes the run branch with the repo's credential. An unresolvable
/// credential reference escalates — a human must grant it — and nothing
/// is pushed.
async fn push_pr(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    let Ok((name, repo)) = primary_repo(&ctx.plan) else {
        return (
            StepOutcome::Failure,
            "primary repo is not in the registry".to_owned(),
            None,
        );
    };
    let Some(secret) = ctx.plan.credentials.get(&repo.credential) else {
        let message = format!(
            "credential `{}` is not resolved; branch `{}` was not pushed",
            repo.credential, wt.branch
        );
        return settle::escalate(ctx, message).await;
    };
    let credential = credential_for(repo.forge, secret.clone());
    if let Err(error) = wt.worktree.push(&repo.url, Some(&credential)).await {
        return (
            StepOutcome::Failure,
            format!("pushing `{}` failed: {error}", wt.branch),
            None,
        );
    }
    open_pr(ctx, wt, name).await
}

/// Opens the PR for the pushed branch, linked to the work item.
async fn open_pr(ctx: &RunCtx, wt: &WtCtx, repo: &str) -> (StepOutcome, String, Option<Pr>) {
    let request = PrRequest {
        repo: repo.to_owned(),
        branch: wt.branch.clone(),
        base: base_branch(&wt.mirror).await,
        title: ctx.plan.item.title.clone(),
        body: format!("{}\n\nCloses {}", ctx.plan.item.body, ctx.plan.item.url),
        item_id: Some(ctx.plan.item.external_id.clone()),
    };
    match ctx.plan.forge.create_pr(&request).await {
        Ok(pr) => (
            StepOutcome::Success,
            format!("opened PR {}", pr.url),
            Some(pr),
        ),
        Err(error) => (
            StepOutcome::Failure,
            format!("opening PR failed: {error}"),
            None,
        ),
    }
}

/// The mirror's default branch: the base PRs target.
async fn base_branch(mirror: &Path) -> String {
    gitcmd::git(&["symbolic-ref", "--short", "HEAD"], mirror, &[])
        .await
        .unwrap_or_else(|_| "main".to_owned())
}
