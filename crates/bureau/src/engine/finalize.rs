//! The `done` terminal: commit, push, open the PR (DESIGN.md section 11
//! step 9). A run that changed nothing exits `NoWork` instead.

use std::path::Path;

use super::ctx::{RunCtx, WtCtx};
use super::machine::primary_repo;
use super::{gitcmd, settle};
use crate::config::Repo;
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

/// The mirror's default branch: the base PRs target.
async fn base_branch(mirror: &Path, clock: fn() -> u64) -> String {
    gitcmd::git(&["symbolic-ref", "--short", "HEAD"], mirror, &[], clock)
        .await
        .unwrap_or_else(|_| "main".to_owned())
}

/// Opens the PR for the pushed branch, linked to the work item.
async fn open_pr(ctx: &RunCtx, wt: &WtCtx, repo: &str) -> (StepOutcome, String, Option<Pr>) {
    let request = PrRequest {
        repo: repo.to_owned(),
        branch: wt.branch.clone(),
        base: base_branch(&wt.mirror, ctx.clock).await,
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

/// Pushes the run branch with the repo's credential. An unresolvable
/// credential reference escalates — a human must grant it — and nothing
/// is pushed; a git failure is data. `None` means the branch landed.
async fn push(ctx: &RunCtx, wt: &WtCtx, repo: &Repo) -> Option<(StepOutcome, String, Option<Pr>)> {
    let Some(secret) = ctx.plan.credentials.get(&repo.credential) else {
        let message = format!(
            "credential `{}` is not resolved; branch `{}` was not pushed",
            repo.credential, wt.branch
        );
        return Some(settle::escalate(ctx, message).await);
    };
    let credential = credential_for(repo.forge, secret.clone());
    let pushed = wt
        .worktree
        .push(&repo.url, Some(&credential), ctx.clock)
        .await;
    pushed.err().map(|error| {
        (
            StepOutcome::Failure,
            format!("pushing `{}` failed: {error}", wt.branch),
            None,
        )
    })
}

/// Pushes the run branch, then opens its PR.
async fn push_pr(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    let Ok((name, repo)) = primary_repo(&ctx.plan) else {
        return (
            StepOutcome::Failure,
            "primary repo is not in the registry".to_owned(),
            None,
        );
    };
    match push(ctx, wt, repo).await {
        Some(done) => done,
        None => open_pr(ctx, wt, name).await,
    }
}

/// `git add -A && git commit` for changes steps left uncommitted.
async fn commit_all(wt: &WtCtx, message: &str, clock: fn() -> u64) -> Result<(), String> {
    gitcmd::git(&["add", "-A"], wt.worktree.path(), &[], clock).await?;
    gitcmd::git(
        &["commit", "-m", message],
        wt.worktree.path(),
        &IDENTITY,
        clock,
    )
    .await?;
    Ok(())
}

/// Commits leftover edits, pushes the branch, and opens the PR.
async fn land(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    let message = format!("bureau: {} ({})", ctx.plan.item.title, ctx.plan.run_id);
    if let Err(error) = commit_all(wt, &message, ctx.clock).await {
        return (StepOutcome::Failure, error, None);
    }
    push_pr(ctx, wt).await
}

/// Whether the worktree moved past its start commit or holds edits.
async fn changed(wt: &WtCtx, clock: fn() -> u64) -> Result<bool, String> {
    let head = gitcmd::git(&["rev-parse", "HEAD"], wt.worktree.path(), &[], clock).await?;
    if head != wt.start_head {
        return Ok(true);
    }
    let status = gitcmd::git(&["status", "--porcelain"], wt.worktree.path(), &[], clock).await?;
    Ok(!status.is_empty())
}

/// Runs the `done` terminal: change detection, commit, push, PR.
pub(super) async fn finalize(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    match changed(wt, ctx.clock).await {
        Ok(false) => (
            StepOutcome::NoWork,
            "run produced no changes".to_owned(),
            None,
        ),
        Ok(true) => land(ctx, wt).await,
        Err(message) => (StepOutcome::Failure, message, None),
    }
}
