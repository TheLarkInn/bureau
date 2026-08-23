//! Opening a run: create its log, or replay an existing one into a
//! finished outcome or a resume context.
//!
//! Resume pins the plan the run started with — including the identities
//! its `run_started` recorded — so re-entry never adopts a newer plan
//! for a run already under way.

use std::path::Path;

use super::context::{self, RunCtx};
use super::{RunOutcome, RunPlan, resume};
use crate::process::Secret;
use crate::runlog::{self, EventKind, RunLog};

/// The open phase's verdict.
pub(super) enum Open {
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
            fallback.identities.clone(),
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
pub(super) fn open(dir: &Path, runs_dir: &Path, plan: &RunPlan) -> Result<Open, String> {
    let secrets: Vec<Secret> = plan.credentials.values().cloned().collect();
    if dir.join(runlog::EVENTS_FILE).exists() {
        resume_open(dir, plan, &secrets)
    } else {
        fresh_open(runs_dir, plan, &secrets)
    }
}
