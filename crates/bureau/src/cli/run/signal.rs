//! Two-stage signal handling for one-shot runs.

use crate::cli::out;
use std::sync::Arc;

use anyhow::Context as _;
use bureau::contract::StepOutcome;
use bureau::engine::{Engine, RunOutcome, RunPlan};
use bureau::state::Store;

use super::super::reconcile::active::{self, Signals, Until};

type Supervised = (RunOutcome, Result<(), bureau::state::Error>);

fn durable_inputs(plan: &RunPlan) -> (bureau::runlog::RunSnapshot, Vec<bureau::process::Secret>) {
    (
        plan.snapshot(),
        plan.credentials.values().cloned().collect(),
    )
}

fn prepare_directory(runs_dir: &std::path::Path, run_id: &str) -> anyhow::Result<()> {
    let directory = bureau::runlog::run_dir(runs_dir, run_id);
    std::fs::create_dir_all(directory).context("creating run directory")
}

fn ensure_started(
    directory: &std::path::Path,
    snapshot: &bureau::runlog::RunSnapshot,
    secrets: &[bureau::process::Secret],
) -> anyhow::Result<()> {
    if bureau::runlog::replay_state(directory).is_ok() {
        return Ok(());
    }
    let events = bureau::runlog::read_events(directory).context("reading cancelled run")?;
    anyhow::ensure!(
        events.is_empty(),
        "non-empty cancelled log has no run_started"
    );
    let mut log = if directory.join(bureau::runlog::EVENTS_FILE).is_file() {
        bureau::runlog::RunLog::resume(directory, secrets)?
    } else {
        let root = directory.parent().context("run directory has no parent")?;
        bureau::runlog::RunLog::create(root, &snapshot.run_id, secrets)?
    };
    log.append(
        bureau::runlog::EventKind::RunStarted,
        bureau::runlog::run_started_snapshot(snapshot),
    )?;
    log.close()?;
    Ok(())
}

fn persist_cancel(
    engine: &Engine,
    store: &Store,
    snapshot: &bureau::runlog::RunSnapshot,
    secrets: &[bureau::process::Secret],
    runs_dir: &std::path::Path,
    directory: &std::path::Path,
) -> anyhow::Result<()> {
    std::fs::write(
        directory.join("CANCEL"),
        "cancelled by second shutdown signal",
    )
    .context("writing cancellation marker")?;
    ensure_started(directory, snapshot, secrets)?;
    engine.block(snapshot, "cancelled by second shutdown signal")?;
    bureau::state::project_run(store, runs_dir, &snapshot.run_id)?;
    Ok(())
}

fn cancel_run(
    engine: &Engine,
    store: &Store,
    owner: Option<&bureau::state::LeaseOwner>,
    snapshot: &bureau::runlog::RunSnapshot,
    secrets: &[bureau::process::Secret],
    runs_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let directory = bureau::runlog::run_dir(runs_dir, &snapshot.run_id);
    let persisted = persist_cancel(engine, store, snapshot, secrets, runs_dir, &directory);
    let released = owner.map_or(Ok(()), bureau::state::LeaseOwner::release);
    persisted?;
    released?;
    Ok(())
}

async fn drain_supervision<F>(
    future: std::pin::Pin<&mut F>,
    signals: &mut Signals,
) -> Option<Supervised>
where
    F: std::future::Future<Output = Supervised> + ?Sized,
{
    out::line(format_args!(
        "run is draining; send a second signal to cancel it"
    ));
    match active::until_signal(future, signals).await {
        Until::Complete(result) => Some(result),
        Until::Signalled => None,
    }
}

async fn await_supervision<F>(
    mut future: std::pin::Pin<&mut F>,
    signals: &mut Signals,
) -> Option<Supervised>
where
    F: std::future::Future<Output = Supervised> + ?Sized,
{
    match active::until_signal(future.as_mut(), signals).await {
        Until::Complete(result) => Some(result),
        Until::Signalled => drain_supervision(future, signals).await,
    }
}

fn cancelled(run_id: String) -> RunOutcome {
    RunOutcome {
        run_id,
        outcome: StepOutcome::Blocked,
        cost_usd: 0.0,
        message: "cancelled by second shutdown signal".to_owned(),
        pr: None,
    }
}

pub(super) async fn run(
    engine: Arc<Engine>,
    store: Arc<Store>,
    plan: RunPlan,
) -> anyhow::Result<RunOutcome> {
    let owner = plan.lease.clone();
    let run_id = plan.run_id.clone();
    let (snapshot, secrets) = durable_inputs(&plan);
    let runs_dir = engine.runs_dir.clone();
    prepare_directory(&runs_dir, &run_id)?;
    let mut signals = Signals::new().context("installing signals")?;
    let mut supervised = Box::pin(bureau::supervise::run(engine.clone(), store.clone(), plan));
    let result = await_supervision(supervised.as_mut(), &mut signals).await;
    let Some((outcome, projection)) = result else {
        drop(supervised);
        tokio::task::yield_now().await;
        cancel_run(
            &engine,
            &store,
            owner.as_ref(),
            &snapshot,
            &secrets,
            &runs_dir,
        )?;
        return Ok(cancelled(run_id));
    };
    projection.context("projecting terminal run state")?;
    Ok(outcome)
}
