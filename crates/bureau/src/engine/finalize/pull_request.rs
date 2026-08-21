//! Idempotent pull-request observation, creation, and recovery.

use super::{
    PublicationError, RunCtx, StepOutcome, WtCtx, control, gitcmd, publication, record_pr,
};
use crate::forge::{Pr, PrRequest};

const OBSERVATION_RESERVE: std::time::Duration = std::time::Duration::from_secs(30);

fn publication_message(observed: PublicationError) -> String {
    match observed {
        PublicationError::Failure(message) | PublicationError::Escalate(message) => message,
    }
}

async fn base_branch(mirror: &std::path::Path) -> String {
    gitcmd::git(&["symbolic-ref", "--short", "HEAD"], mirror, &[])
        .await
        .unwrap_or_else(|_| "main".to_owned())
}

async fn observed(ctx: &RunCtx, repo: &str, branch: &str) -> Result<Option<Pr>, PublicationError> {
    let result = tokio::time::timeout(ctx.remaining(), ctx.plan.forge.open_prs(repo, branch))
        .await
        .map_err(|_| PublicationError::Escalate(control::deadline_message(ctx)))?;
    let prs = result.map_err(|error| {
        PublicationError::Failure(format!("observing pull requests failed: {error}"))
    })?;
    Ok(prs.into_iter().find(|pr| pr.branch == branch))
}

fn finish_timeout(
    ctx: &RunCtx,
    commit: &str,
    observation: Result<Option<Pr>, PublicationError>,
) -> (StepOutcome, String, Option<Pr>) {
    match observation {
        Ok(Some(pr)) => record_pr(ctx, pr, commit),
        Ok(None) => publication::stop(PublicationError::Escalate(
            "pull request creation timed out".to_owned(),
        )),
        Err(error) => publication::stop(error),
    }
}

async fn recover_timeout(
    ctx: &RunCtx,
    repo: &str,
    branch: &str,
    commit: &str,
) -> (StepOutcome, String, Option<Pr>) {
    let observation = observed(ctx, repo, branch).await;
    finish_timeout(ctx, commit, observation)
}

fn finish_recovery(
    ctx: &RunCtx,
    commit: &str,
    error: &crate::forge::Error,
    observation: Result<Option<Pr>, PublicationError>,
) -> (StepOutcome, String, Option<Pr>) {
    match observation {
        Ok(None) => (
            StepOutcome::Failure,
            format!("opening PR failed: {error}"),
            None,
        ),
        Ok(Some(pr)) => record_pr(ctx, pr, commit),
        Err(PublicationError::Escalate(message)) => publication::stop(PublicationError::Escalate(
            format!("opening PR was ambiguous: {error}; {message}"),
        )),
        Err(observed) => {
            let detail = publication_message(observed);
            publication::stop(PublicationError::Escalate(format!(
                "opening PR was ambiguous: {error}; {detail}"
            )))
        }
    }
}

async fn recover_create(
    ctx: &RunCtx,
    repo: &str,
    branch: &str,
    commit: &str,
    error: crate::forge::Error,
) -> (StepOutcome, String, Option<Pr>) {
    let observation = observed(ctx, repo, branch).await;
    finish_recovery(ctx, commit, &error, observation)
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

async fn finish_create_wait(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
    result: Result<Result<Pr, crate::forge::Error>, tokio::time::error::Elapsed>,
) -> (StepOutcome, String, Option<Pr>) {
    match result {
        Ok(result) => finish_create(ctx, repo, &wt.branch, commit, result).await,
        Err(_) => recover_timeout(ctx, repo, &wt.branch, commit).await,
    }
}

async fn create_checked(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
    timeout: std::time::Duration,
) -> (StepOutcome, String, Option<Pr>) {
    let request = request(ctx, wt, repo).await;
    let result = tokio::time::timeout(timeout, ctx.plan.forge.create_pr(&request)).await;
    finish_create_wait(ctx, wt, repo, commit, result).await
}

async fn create_result(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
) -> Result<(StepOutcome, String, Option<Pr>), PublicationError> {
    publication::check(ctx).await?;
    let timeout = ctx
        .remaining()
        .checked_sub(OBSERVATION_RESERVE)
        .ok_or_else(|| PublicationError::Escalate(control::deadline_message(ctx)))?;
    Ok(create_checked(ctx, wt, repo, commit, timeout).await)
}

async fn create(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
) -> (StepOutcome, String, Option<Pr>) {
    match create_result(ctx, wt, repo, commit).await {
        Ok(result) => result,
        Err(error) => publication::stop(error),
    }
}

async fn finish_observation(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
    observation: Result<Option<Pr>, PublicationError>,
) -> (StepOutcome, String, Option<Pr>) {
    match observation {
        Ok(Some(pr)) => record_pr(ctx, pr, commit),
        Ok(None) => create(ctx, wt, repo, commit).await,
        Err(error) => publication::stop(error),
    }
}

pub(super) async fn open(
    ctx: &RunCtx,
    wt: &WtCtx,
    repo: &str,
    commit: &str,
) -> (StepOutcome, String, Option<Pr>) {
    let observation = observed(ctx, repo, &wt.branch).await;
    finish_observation(ctx, wt, repo, commit, observation).await
}
