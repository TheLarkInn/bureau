//! Run setup: open or resume the log, cut the worktree, hand the run to
//! the machine, and settle the outcome.

use std::path::{Path, PathBuf};

use super::context::{self, RunCtx, WtCtx};
use super::machine::{Stop, primary_repo, run_loop};
use super::{RunOutcome, RunPlan, finalize, gitcmd, plugins, resume, settle, stream};
use crate::contract::StepOutcome;
use crate::git::{CheckoutCache, Worktree, credential_for};
use crate::process::Secret;
use crate::runlog::RunLog;
use crate::runlog::{self, EventKind};

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
    let data = runlog::run_started_snapshot(&ctx.plan.snapshot());
    stream::lock(&ctx.log)
        .append(EventKind::RunStarted, data)
        .map_err(|e| format!("appending run_started: {e}"))?;
    ctx.started = true;
    Ok(())
}

fn setup_failure(mut ctx: RunCtx, message: String) -> RunOutcome {
    if let Err(error) = append_started(&mut ctx) {
        return RunOutcome::bare(&ctx.plan.run_id, StepOutcome::Failure, error);
    }
    settle::finish(ctx, StepOutcome::Failure, message, None)
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

/// Resolves a non-pause stop into the settle triple.
async fn resolve_stop(
    ctx: &RunCtx,
    wt: &WtCtx,
    stop: Stop,
) -> (StepOutcome, String, Option<crate::forge::Pr>) {
    match stop {
        Stop::Done => finalize::finalize(ctx, wt).await,
        Stop::Fail(message) => (StepOutcome::Failure, message, None),
        Stop::Escalate(message) => settle::escalate(ctx, message).await,
        Stop::Pause => unreachable!("handled by end_run"),
    }
}

/// Resolves the machine's stop reason into the run's outcome. The
/// worktree guard drops here, before the log closes.
async fn end_run(ctx: RunCtx, wt: WtCtx, stop: Stop) -> RunOutcome {
    if matches!(stop, Stop::Pause) {
        drop(wt);
        return settle::paused(ctx);
    }
    let (outcome, message, pr) = resolve_stop(&ctx, &wt, stop).await;
    drop(wt);
    settle::finish(ctx, outcome, message, pr)
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

/// Runs the worktree phase, the machine loop, and the settle phase.
async fn run_to_terminal(cache: &CheckoutCache, ctx: RunCtx) -> RunOutcome {
    match worktree_phase(cache, &ctx).await {
        Ok(wt) => with_worktree(ctx, wt).await,
        Err(message) => setup_failure(ctx, message),
    }
}

/// The open phase's verdict.
enum Open {
    /// The log holds a finished run; return its outcome untouched.
    Finished(RunOutcome),
    /// The machine runs from the replayed state.
    Running(Box<RunCtx>),
}

/// Creates a run's log and records `run_started` first.
fn fresh_open(runs_dir: &Path, plan: &RunPlan, secrets: &[Secret]) -> Result<Open, String> {
    let log = RunLog::create(runs_dir, &plan.run_id, secrets)
        .map_err(|e| format!("creating run log: {e}"))?;
    let history = resume::fresh(resume::entry(&plan.pipeline), false);
    Ok(Open::Running(Box::new(context::run_ctx(
        plan, log, history,
    ))))
}

fn pinned_plan(events: &[runlog::Event], fallback: &RunPlan) -> RunPlan {
    let snapshot = events
        .iter()
        .find(|event| event.kind == EventKind::RunStarted)
        .and_then(|event| serde_json::from_value::<runlog::RunStartedData>(event.data.clone()).ok())
        .and_then(|started| started.snapshot);
    let Some(snapshot) = snapshot else {
        return fallback.clone();
    };
    if fallback.config_source.is_some() {
        let mut plan = super::rehydrate(
            snapshot,
            fallback.forge.clone(),
            fallback.credentials.clone(),
        );
        plan.lease.clone_from(&fallback.lease);
        return plan;
    }
    let mut plan = fallback.clone();
    plan.plugin_sources = snapshot.plugin_sources;
    plan
}

/// Opens a log for appending and assembles the resume context.
fn resume_ctx(
    dir: &Path,
    plan: &RunPlan,
    secrets: &[Secret],
    history: resume::History,
) -> Result<Open, String> {
    let log = RunLog::resume(dir, secrets).map_err(|e| format!("opening run log: {e}"))?;
    Ok(Open::Running(Box::new(context::run_ctx(
        plan, log, history,
    ))))
}

/// Replays an existing run's log into a finished outcome or a resume.
fn resume_open(dir: &Path, plan: &RunPlan, secrets: &[Secret]) -> Result<Open, String> {
    let events = runlog::read_events(dir).map_err(|e| format!("reading run log: {e}"))?;
    let pinned = pinned_plan(&events, plan);
    match resume::replay(events, &pinned.pipeline) {
        resume::Replay::Finished(data) => {
            Ok(Open::Finished(RunOutcome::finished(&pinned.run_id, data)))
        }
        resume::Replay::Resume(history) => resume_ctx(dir, &pinned, secrets, history),
    }
}

/// Opens the run fresh or resumes it from its event log.
fn open(dir: &Path, runs_dir: &Path, plan: &RunPlan) -> Result<Open, String> {
    let secrets: Vec<Secret> = plan.credentials.values().cloned().collect();
    if dir.join(runlog::EVENTS_FILE).exists() {
        resume_open(dir, plan, &secrets)
    } else {
        fresh_open(runs_dir, plan, &secrets)
    }
}

/// Runs a pipeline to a terminal, creating or resuming the run's log.
pub(super) async fn run(runs_dir: &Path, cache: &CheckoutCache, plan: &RunPlan) -> RunOutcome {
    let dir = runlog::run_dir(runs_dir, &plan.run_id);
    match open(&dir, runs_dir, plan) {
        Ok(Open::Finished(outcome)) => outcome,
        Ok(Open::Running(ctx)) => run_to_terminal(cache, *ctx).await,
        Err(message) => RunOutcome::bare(&plan.run_id, StepOutcome::Failure, message),
    }
}
