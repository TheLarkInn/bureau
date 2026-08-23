//! Run setup: verify the run's credentials, cut the worktree, hand the
//! run to the machine, and settle the outcome.

use std::path::{Path, PathBuf};

use super::context::{self, RunCtx, WtCtx};
use super::machine::{Stop, primary_repo, run_loop};
use super::{RunOutcome, RunPlan, finalize, gitcmd, identity, open, plugins, settle, stream};
use crate::contract::StepOutcome;
use crate::git::{CheckoutCache, Worktree, credential_for};
use crate::runlog::{self, EventKind, RunTerminal};

/// The run branch: `<branch_prefix><pipeline>/<run_id>`.
fn branch_name(plan: &RunPlan) -> String {
    format!(
        "{}{}/{}",
        plan.assignment.branch_prefix, plan.pipeline.name, plan.run_id
    )
}

/// Removes a stale worktree registration and branch so resume can
/// re-create `wt/` fresh; failures mean there was nothing to clear.
async fn clear_stale(mirror: &Path, wt_dir: &Path, branch: &str) {
    let dir = wt_dir.to_string_lossy().into_owned();
    let _ = gitcmd::git(&["worktree", "remove", "--force", &dir], mirror, &[]).await;
    let _ = gitcmd::git(&["worktree", "prune"], mirror, &[]).await;
    let _ = gitcmd::git(&["branch", "-D", branch], mirror, &[]).await;
}

async fn create_worktree(
    mirror: &Path,
    directory: &Path,
    branch: &str,
) -> Result<(Worktree, String), String> {
    let worktree = Worktree::create(mirror, directory, branch, false)
        .await
        .map_err(|error| format!("creating worktree failed: {error}"))?;
    let head = gitcmd::git(&["rev-parse", "HEAD"], worktree.path(), &[])
        .await
        .map_err(|error| format!("reading worktree HEAD failed: {error}"))?;
    Ok((worktree, head))
}

async fn restore_checkpoint(worktree: &Worktree, commit: Option<&str>) -> Result<(), String> {
    if let Some(commit) = commit {
        gitcmd::git(&["reset", "--hard", commit], worktree.path(), &[])
            .await
            .map_err(|error| format!("restoring checkpoint failed: {error}"))?;
    }
    Ok(())
}

/// Ensures the mirror is fresh and clears stale worktree state for the
/// run branch, returning `(mirror, branch, worktree dir)`.
async fn prepare(
    cache: &CheckoutCache,
    ctx: &RunCtx,
) -> Result<(PathBuf, String, PathBuf), String> {
    let (name, repo) = primary_repo(&ctx.plan)?;
    let credential = ctx
        .plan
        .credentials
        .get(&repo.credential)
        .map(|secret| credential_for(repo.forge, secret.clone()));
    let mirror = cache
        .mirror(&repo.url, credential.as_ref())
        .await
        .map_err(|e| format!("mirroring `{name}` failed: {e}"))?;
    let branch = branch_name(&ctx.plan);
    let wt_dir = stream::lock(&ctx.log).dir().join("wt");
    clear_stale(&mirror, &wt_dir, &branch).await;
    Ok((mirror, branch, wt_dir))
}

/// Cuts (or re-cuts) the worktree and records its start commit.
async fn worktree_phase(cache: &CheckoutCache, ctx: &RunCtx) -> Result<WtCtx, String> {
    let (mirror, branch, wt_dir) = prepare(cache, ctx).await?;
    let (worktree, created_head) = create_worktree(&mirror, &wt_dir, &branch).await?;
    restore_checkpoint(&worktree, ctx.checkpoint.as_deref()).await?;
    let start_head = ctx.base_commit.clone().unwrap_or(created_head);
    Ok(WtCtx {
        worktree,
        mirror,
        branch,
        start_head,
    })
}

/// Appends the `run_started` event every run begins with.
fn append_started(ctx: &mut RunCtx) -> Result<(), String> {
    if ctx.started {
        return Ok(());
    }
    if let Some(reason) = context::ownership_reason(ctx) {
        return Err(reason);
    }
    let data = runlog::run_started_snapshot(&ctx.plan.snapshot(), &ctx.verified);
    stream::lock(&ctx.log)
        .append(EventKind::RunStarted, data)
        .map_err(|e| format!("appending run_started: {e}"))?;
    ctx.started = true;
    Ok(())
}

async fn setup_failure(mut ctx: RunCtx, message: String) -> RunOutcome {
    if let Err(error) = append_started(&mut ctx) {
        return RunOutcome::bare(&ctx.plan.run_id, StepOutcome::Failure, error);
    }
    let raw = (StepOutcome::Failure, message, None);
    let result = settle::project(&ctx, RunTerminal::Abort, raw).await;
    settle::finish(ctx, result)
}

fn prepare_plugins(ctx: &mut RunCtx, wt: &WtCtx) -> Result<(), plugins::PrepareError> {
    if ctx.started {
        return Ok(());
    }
    let run_dir = stream::lock(&ctx.log).dir().to_path_buf();
    plugins::prepare(&mut ctx.plan, &run_dir, wt.worktree.path())
}

fn prepare_stop(error: plugins::PrepareError) -> Stop {
    match error {
        plugins::PrepareError::Failure(message) => Stop::Fail(message),
        plugins::PrepareError::Blocked(message) => Stop::Escalate(message),
    }
}

const fn done_terminal(outcome: StepOutcome) -> RunTerminal {
    match outcome {
        StepOutcome::Success | StepOutcome::NoWork => RunTerminal::Done,
        StepOutcome::Failure => RunTerminal::Abort,
        StepOutcome::Blocked => RunTerminal::Escalate,
    }
}

/// Resolves a non-pause stop into the settle triple.
async fn resolve_stop(ctx: &RunCtx, wt: &WtCtx, stop: Stop) -> settle::TerminalResult {
    let (terminal, raw) = match stop {
        Stop::Done => {
            let raw = finalize::finalize(ctx, wt).await;
            (done_terminal(raw.0), raw)
        }
        Stop::Fail(message) => (RunTerminal::Abort, (StepOutcome::Failure, message, None)),
        Stop::Escalate(message) => (RunTerminal::Escalate, (StepOutcome::Blocked, message, None)),
        Stop::Pause => unreachable!("handled by end_run"),
    };
    settle::project(ctx, terminal, raw).await
}

/// Resolves the machine's stop reason into the run's outcome. The
/// worktree guard drops here, before the log closes.
async fn end_run(ctx: RunCtx, wt: WtCtx, stop: Stop) -> RunOutcome {
    if matches!(stop, Stop::Pause) {
        drop(wt);
        return settle::paused(ctx);
    }
    let result = resolve_stop(&ctx, &wt, stop).await;
    drop(wt);
    settle::finish(ctx, result)
}

async fn run_prepared(mut ctx: RunCtx, wt: WtCtx) -> RunOutcome {
    let stop = run_loop(&mut ctx, &wt).await;
    end_run(ctx, wt, stop).await
}

async fn start_prepared(
    ctx: RunCtx,
    wt: WtCtx,
    prepared: Result<(), plugins::PrepareError>,
) -> RunOutcome {
    let Some(error) = prepared.err() else {
        return run_prepared(ctx, wt).await;
    };
    end_run(ctx, wt, prepare_stop(error)).await
}

/// Drives the machine loop with its worktree, then settles.
async fn with_worktree(mut ctx: RunCtx, wt: WtCtx) -> RunOutcome {
    let prepared = prepare_plugins(&mut ctx, &wt);
    if let Err(error) = append_started(&mut ctx) {
        return RunOutcome::bare(&ctx.plan.run_id, StepOutcome::Failure, error);
    }
    start_prepared(ctx, wt, prepared).await
}

async fn finish_worktree(ctx: RunCtx, result: Result<WtCtx, String>) -> RunOutcome {
    match result {
        Ok(wt) => with_worktree(ctx, wt).await,
        Err(message) => setup_failure(ctx, message).await,
    }
}

/// Runs the worktree phase, the machine loop, and the settle phase.
async fn run_verified(cache: &CheckoutCache, ctx: RunCtx) -> RunOutcome {
    let result = worktree_phase(cache, &ctx).await;
    finish_worktree(ctx, result).await
}

/// Runs the verified run, or settles the failed check as a setup abort.
async fn finish_checked(
    cache: &CheckoutCache,
    ctx: RunCtx,
    checked: Result<(), String>,
) -> RunOutcome {
    match checked {
        Ok(()) => Box::pin(run_verified(cache, ctx)).await,
        Err(message) => setup_failure(ctx, message).await,
    }
}

/// The identity check comes first: a credential the forge rejects, or
/// one that is a different account than the settings declare, aborts the
/// run before anything spawns.
async fn run_to_terminal(cache: &CheckoutCache, mut ctx: RunCtx) -> RunOutcome {
    let checked = identity::verify(&mut ctx).await;
    Box::pin(finish_checked(cache, ctx, checked)).await
}

/// Runs a pipeline to a terminal, creating or resuming the run's log.
pub(super) async fn run(runs_dir: &Path, cache: &CheckoutCache, plan: &RunPlan) -> RunOutcome {
    let dir = runlog::run_dir(runs_dir, &plan.run_id);
    match open::open(&dir, runs_dir, plan) {
        Ok(open::Open::Finished(outcome)) => outcome,
        Ok(open::Open::Running(ctx)) => Box::pin(run_to_terminal(cache, *ctx)).await,
        Err(message) => RunOutcome::bare(&plan.run_id, StepOutcome::Failure, message),
    }
}
