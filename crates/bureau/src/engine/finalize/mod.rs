//! Idempotent `done`: checkpoint, push, and create or adopt a PR.
mod publication;
mod pull_request;

use super::context::{self, RunCtx, WtCtx};
use super::machine::primary_repo;
use super::{control, gitcmd};
use crate::config::Repo;
use crate::contract::StepOutcome;
use crate::forge::Pr;
use crate::git::{Credential, credential_for};
use crate::runlog::EventKind;

/// A failed publication gate or push: either a hard failure or an
/// ambiguity a human must resolve.
type PublicationError = publication::Error;

pub(super) fn opened(pr: Pr) -> (StepOutcome, String, Option<Pr>) {
    (
        StepOutcome::Success,
        format!("opened PR {}", pr.url),
        Some(pr),
    )
}

pub(super) fn record_pr(ctx: &RunCtx, pr: Pr, commit: &str) -> (StepOutcome, String, Option<Pr>) {
    context::append(
        ctx,
        EventKind::PrCreated,
        crate::runlog::pr_created(&pr, commit),
    );
    opened(pr)
}

async fn changed(wt: &WtCtx) -> Result<bool, String> {
    let head = gitcmd::git(&["rev-parse", "HEAD"], wt.worktree.path(), &[]).await?;
    if head != wt.start_head {
        return Ok(true);
    }
    let status = gitcmd::git(&["status", "--porcelain"], wt.worktree.path(), &[]).await?;
    Ok(!status.is_empty())
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

fn landing_target<'a>(
    ctx: &'a RunCtx,
    wt: &WtCtx,
) -> Result<(&'a str, &'a Repo, Credential), PublicationError> {
    let (name, repo) = primary_repo(&ctx.plan).map_err(PublicationError::Failure)?;
    let secret = ctx.plan.credentials.get(&repo.credential).ok_or_else(|| {
        PublicationError::Escalate(format!(
            "credential `{}` is not resolved; branch `{}` was not pushed",
            repo.credential, wt.branch
        ))
    })?;
    Ok((name, repo, credential_for(repo.forge, secret.clone())))
}

async fn push_if_needed(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &Repo,
    credential: &Credential,
    commit: &str,
) -> Result<(), PublicationError> {
    if ctx.pushed_commit.as_deref() == Some(commit) {
        return Ok(());
    }
    let pushed = tokio::time::timeout(
        ctx.remaining(),
        wt.worktree.push(&repo.url, Some(credential)),
    )
    .await
    .map_err(|_| PublicationError::Escalate(control::deadline_message(ctx)))?;
    pushed.map_err(|error| {
        PublicationError::Failure(format!("pushing `{}` failed: {error}", wt.branch))
    })?;
    publication::check(ctx).await?;
    context::append(
        ctx,
        EventKind::BranchPushed,
        crate::runlog::branch_pushed(&wt.branch, commit),
    );
    Ok(())
}

async fn push_branch(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &Repo,
    credential: &Credential,
) -> Result<String, PublicationError> {
    publication::check(ctx).await?;
    let commit = gitcmd::git(&["rev-parse", "HEAD"], wt.worktree.path(), &[])
        .await
        .map_err(PublicationError::Failure)?;
    push_if_needed(ctx, wt, repo, credential, &commit).await?;
    Ok(commit)
}

async fn finish_push(
    ctx: &RunCtx,
    wt: &WtCtx,
    name: &str,
    pushed: Result<String, PublicationError>,
) -> (StepOutcome, String, Option<Pr>) {
    match pushed {
        Ok(commit) => pull_request::open(ctx, wt, name, &commit).await,
        Err(error) => publication::stop(ctx, error).await,
    }
}

async fn push_target(
    ctx: &RunCtx,
    wt: &WtCtx,
    target: (&str, &Repo, Credential),
) -> (StepOutcome, String, Option<Pr>) {
    let (name, repo, credential) = target;
    let pushed = push_branch(ctx, wt, repo, &credential).await;
    finish_push(ctx, wt, name, pushed).await
}

async fn push_pr(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    let target = match landing_target(ctx, wt) {
        Ok(target) => target,
        Err(error) => return publication::stop(ctx, error).await,
    };
    push_target(ctx, wt, target).await
}

async fn land_result(
    ctx: &RunCtx,
    wt: &WtCtx,
) -> Result<(StepOutcome, String, Option<Pr>), PublicationError> {
    publication::check(ctx).await?;
    let message = format!("bureau: {} ({})", ctx.plan.item.title, ctx.plan.run_id);
    commit_all(wt, &message)
        .await
        .map_err(PublicationError::Failure)?;
    Ok(push_pr(ctx, wt).await)
}

async fn land(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    match land_result(ctx, wt).await {
        Ok(result) => result,
        Err(error) => publication::stop(ctx, error).await,
    }
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

pub(super) async fn finalize(ctx: &RunCtx, wt: &WtCtx) -> (StepOutcome, String, Option<Pr>) {
    if let Some(pr) = &ctx.pr {
        return opened(pr.clone());
    }
    finalize_new(ctx, wt).await
}
