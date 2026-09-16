//! Factory logs are read without repair; any opening or torn-tail repair is lease-fenced.

use std::io;
use std::path::Path;

use crate::process::Secret;
use crate::runlog::{self, EventKind, RunLog};

use super::super::context::{self, RunCtx};
use super::super::{RunOutcome, RunPlan, resume};
use super::factory;

pub(super) enum Open {
    Finished(RunOutcome),
    Running(Box<RunCtx>),
}

fn durable(runs: &Path, plan: &RunPlan, secrets: &[Secret]) -> io::Result<RunLog> {
    let log = RunLog::create_events(runs, &plan.run_id, secrets)?;
    std::fs::create_dir_all(log.dir().join("artifacts"))?;
    std::fs::create_dir_all(log.dir().join("wt"))?;
    std::fs::File::open(log.dir())?.sync_all()?;
    Ok(log)
}

fn create(runs: &Path, plan: &RunPlan, secrets: &[Secret]) -> Result<RunLog, String> {
    let action = || {
        if factory::configured(plan) {
            durable(runs, plan, secrets)
        } else {
            RunLog::create(runs, &plan.run_id, secrets)
        }
    };
    factory::owned(plan, action)
}

fn fresh(runs: &Path, plan: &RunPlan, secrets: &[Secret]) -> Result<Open, String> {
    let log = create(runs, plan, secrets)?;
    let history = resume::fresh(resume::entry(&plan.pipeline), false);
    Ok(Open::Running(Box::new(context::run_ctx(
        plan, log, history,
    ))))
}

fn snapshot(events: &[runlog::Event]) -> Option<runlog::RunSnapshot> {
    events
        .iter()
        .find(|event| event.kind == EventKind::RunStarted)
        .and_then(|event| serde_json::from_value::<runlog::RunStartedData>(event.data.clone()).ok())
        .and_then(|started| started.snapshot)
}

fn pinned_plan(events: &[runlog::Event], fallback: &RunPlan) -> RunPlan {
    let Some(snapshot) = snapshot(events) else {
        return fallback.clone();
    };
    let factory = snapshot
        .pipeline
        .steps
        .iter()
        .any(|step| step.copilot_factory.is_some());
    if fallback.config_source.is_some() || factory || factory::configured(fallback) {
        let mut plan = super::super::rehydrate(
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

fn resumed(
    dir: &Path,
    plan: &RunPlan,
    secrets: &[Secret],
    history: resume::History,
) -> Result<Open, String> {
    let log = factory::owned(plan, || RunLog::resume(dir, secrets))?;
    Ok(Open::Running(Box::new(context::run_ctx(
        plan, log, history,
    ))))
}

fn replay(dir: &Path, plan: &RunPlan, secrets: &[Secret]) -> Result<Open, String> {
    let events = runlog::read_events_tolerant(dir).map_err(|error| error.to_string())?;
    let records = crate::runlog::copilot_factory::Records::replay(&events)
        .map_err(|error| error.to_string())?;
    let pinned = pinned_plan(&events, plan);
    match resume::replay(events, &pinned.pipeline) {
        resume::Replay::Finished(data) => {
            if !records
                .0
                .values()
                .all(crate::runlog::copilot_factory::Record::can_clean)
            {
                return Err(
                    "terminal run log contains an unsettled local factory; workspace retained"
                        .into(),
                );
            }
            Ok(Open::Finished(RunOutcome::finished(&pinned.run_id, data)))
        }
        resume::Replay::Resume(mut history) => {
            history.factories = records;
            resumed(dir, &pinned, secrets, *history)
        }
    }
}

pub(super) fn open(dir: &Path, runs: &Path, plan: &RunPlan) -> Result<Open, String> {
    let secrets = super::super::copilot_factory::secrets(plan);
    if dir.join(runlog::EVENTS_FILE).exists() {
        replay(dir, plan, &secrets)
    } else {
        fresh(runs, plan, &secrets)
    }
}

pub(super) fn append_started(ctx: &mut RunCtx) -> Result<(), String> {
    if ctx.started {
        return Ok(());
    }
    if let Some(reason) = context::ownership_reason(ctx) {
        return Err(reason);
    }
    let data = runlog::run_started_snapshot(&ctx.plan.snapshot());
    factory::owned(&ctx.plan, || {
        super::super::stream::lock(&ctx.log)
            .append(EventKind::RunStarted, data)
            .map(|_| ())
            .map_err(io::Error::other)
    })?;
    ctx.started = true;
    Ok(())
}
