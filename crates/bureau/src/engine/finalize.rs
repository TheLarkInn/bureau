//! Idempotent `done`: checkpoint, push, and create or adopt one PR.

use std::path::Path;

use super::machine::{self, RunCtx, WtCtx, primary_repo};
use super::{gitcmd, settle};
use crate::config::Repo;
use crate::contract::StepOutcome;
use crate::forge::{Pr, PrRequest};
use crate::git::{Credential, credential_for};
use crate::runlog::EventKind;

pub(super) async fn finalize(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    if let Some(pr) = &ctx.pr {
        return opened(pr.clone());
    }
    finalize_new(ctx, wt).await
}

async fn finalize_new(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
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

async fn changed(wt: &WtCtx) -> Result<bool, String> {
    let head = gitcmd::git(&["rev-parse", "HEAD"], wt.worktree.path(), &[]).await?;
    if head != wt.start_head {
        return Ok(true);
    }
    let status = gitcmd::git(&["status", "--porcelain"], wt.worktree.path(), &[]).await?;
    Ok(!status.is_empty())
}

async fn land(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    let message = format!("bureau: {} ({})", ctx.plan.item.title, ctx.plan.run_id);
    if let Err(error) = commit_all(wt, &message).await {
        return (StepOutcome::Failure, error, None);
    }
    push_pr(ctx, wt).await
}

async fn commit_all(wt: &WtCtx, message: &str) -> Result<(), String> {
    let status = gitcmd::git(&["status", "--porcelain"], wt.worktree.path(), &[]).await?;
    if status.is_empty() {
        return Ok(());
    }
    gitcmd::git(&["add", "-A"], wt.worktree.path(), &[]).await?;
    gitcmd::git(
        &["commit", "-m", message],
        wt.worktree.path(),
        &gitcmd::IDENTITY,
    )
    .await?;
    Ok(())
}

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
    match push_branch(ctx, wt, repo, &credential).await {
        Ok(commit) => open_pr(ctx, wt, name, &commit).await,
        Err(error) => (StepOutcome::Failure, error, None),
    }
}

async fn push_branch(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &Repo,
    credential: &Credential,
) -> Result<String, String> {
    let commit = gitcmd::git(&["rev-parse", "HEAD"], wt.worktree.path(), &[]).await?;
    if ctx.pushed_commit.as_deref() != Some(&commit) {
        wt.worktree
            .push(&repo.url, Some(credential))
            .await
            .map_err(|error| format!("pushing `{}` failed: {error}", wt.branch))?;
        let data = crate::runlog::branch_pushed(&wt.branch, &commit);
        machine::append(ctx, EventKind::BranchPushed, data);
    }
    Ok(commit)
}

async fn open_pr(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
) -> (StepOutcome, String, Option<Pr>) {
    if let Some(pr) = observed(ctx, repo, &wt.branch).await {
        return record_pr(ctx, pr, commit);
    }
    create_pr(ctx, wt, repo, commit).await
}

async fn create_pr(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
) -> (StepOutcome, String, Option<Pr>) {
    let request = request(ctx, wt, repo).await;
    let result = ctx.plan.forge.create_pr(&request).await;
    finish_create(ctx, repo, &wt.branch, commit, result).await
}

async fn finish_create(
    ctx: &RunCtx,
    repo: &str,
    branch: &str,
    commit: &str,
    result: Result<Pr, crate::forge::Error>,
) -> (StepOutcome, String, Option<Pr>) {
    match result {
        Ok(pr) => record_pr(ctx, pr, commit),
        Err(error) => recover_create(ctx, repo, branch, commit, error).await,
    }
}

async fn request(ctx: &RunCtx, wt: &WtCtx, repo: &str) -> PrRequest {
    PrRequest {
        repo: repo.to_owned(),
        branch: wt.branch.clone(),
        base: base_branch(&wt.mirror).await,
        title: ctx.plan.item.title.clone(),
        body: format!("{}\n\nCloses {}", ctx.plan.item.body, ctx.plan.item.url),
        item_id: Some(ctx.plan.item.external_id.clone()),
    }
}

async fn observed(ctx: &RunCtx, repo: &str, branch: &str) -> Option<Pr> {
    ctx.plan
        .forge
        .open_prs(repo, branch)
        .await
        .ok()?
        .into_iter()
        .find(|pr| pr.branch == branch)
}

async fn recover_create(
    ctx: &RunCtx,
    repo: &str,
    branch: &str,
    commit: &str,
    error: crate::forge::Error,
) -> (StepOutcome, String, Option<Pr>) {
    observed(ctx, repo, branch).await.map_or_else(
        || {
            (
                StepOutcome::Failure,
                format!("opening PR failed: {error}"),
                None,
            )
        },
        |pr| record_pr(ctx, pr, commit),
    )
}

fn record_pr(ctx: &RunCtx, pr: Pr, commit: &str) -> (StepOutcome, String, Option<Pr>) {
    machine::append(
        ctx,
        EventKind::PrCreated,
        crate::runlog::pr_created(&pr, commit),
    );
    opened(pr)
}

fn opened(pr: Pr) -> (StepOutcome, String, Option<Pr>) {
    (
        StepOutcome::Success,
        format!("opened PR {}", pr.url),
        Some(pr),
    )
}

async fn base_branch(mirror: &Path) -> String {
    gitcmd::git(&["symbolic-ref", "--short", "HEAD"], mirror, &[])
        .await
        .unwrap_or_else(|_| "main".to_owned())
}
