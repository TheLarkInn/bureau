//! `run` and `retry`: claim one work item and drive one pipeline run to
//! a terminal (DESIGN.md sections 11 and 13).
mod committed;
mod signal;

use crate::cli::out;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;

use bureau::config::{Assignment, Config};
use bureau::contract::StepOutcome;
use bureau::engine::{Engine, RunOutcome, RunPlan, new_run_id};
use bureau::forge::{Forge, Item};
use bureau::process::Secret;
use bureau::runlog::{self, EventKind, RunStartedData};
use bureau::state::{LeaseOwner, Store};

use super::{Paths, prepare};

struct Prepared {
    forge: Arc<dyn Forge>,
    item: Item,
    credentials: BTreeMap<String, Secret>,
}

/// The one assignment bound to `pipeline`; v0 runs exactly one, so zero
/// matches and ambiguity are both errors that name what was found.
fn by_pipeline<'c>(config: &'c Config, pipeline: &str) -> Result<&'c Assignment, i32> {
    let found: Vec<&Assignment> = config
        .assignments
        .values()
        .filter(|a| a.pipeline == pipeline)
        .collect();
    match found.as_slice() {
        [] => {
            out::error(format_args!("no assignment uses pipeline `{pipeline}`"));
            Err(2)
        }
        [one] => Ok(one),
        many => {
            let names = many
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            out::error(format_args!(
                "pipeline `{pipeline}` is used by {} assignments: {names}",
                many.len()
            ));
            Err(2)
        }
    }
}

/// What `retry` re-runs: the original run's assignment name and item,
/// read from its `run_started` event.
fn retry_target(runs: &Path, run_id: &str) -> anyhow::Result<Option<(String, String)>> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        out::error(format_args!("no such run: `{run_id}`"));
        return Ok(None);
    }
    let events = runlog::read_events(&dir).context("reading run events")?;
    let started = events
        .iter()
        .find(|e| e.kind == EventKind::RunStarted)
        .and_then(|e| serde_json::from_value::<RunStartedData>(e.data.clone()).ok());
    let target = started.and_then(|d| d.item.map(|item| (d.assignment, item)));
    if let Some(target) = target {
        return Ok(Some(target));
    }
    out::error(format_args!(
        "run `{run_id}` recorded no work item; nothing to retry"
    ));
    Ok(None)
}

async fn approved(
    forge: &dyn Forge,
    assignment: &Assignment,
    item_query: &str,
) -> anyhow::Result<Option<Item>> {
    let Some(item) = prepare::find_item(forge, assignment, item_query).await? else {
        out::error(format_args!(
            "no item `{item_query}` in `{}`",
            assignment.work.source
        ));
        return Ok(None);
    };
    let Some(item) = bureau::reconcile::approved_item(assignment, item) else {
        out::error(format_args!(
            "item `{item_query}` is missing the required approval label"
        ));
        return Ok(None);
    };
    Ok(Some(item))
}

async fn prepare_execution(
    config: &Config,
    assignment: &Assignment,
    settings: &bureau::setup::Settings,
    item_query: &str,
) -> anyhow::Result<Option<Prepared>> {
    let Some(credentials) = prepare::resolve_credentials(config, assignment, settings) else {
        return Ok(None);
    };
    let forge = prepare::work_forge(config, assignment, &credentials)?;
    let Some(item) = approved(&*forge, assignment, item_query).await? else {
        return Ok(None);
    };
    Ok(Some(Prepared {
        forge,
        item,
        credentials,
    }))
}

/// Claims the item under the shared renewable lease policy.
fn claim(
    store: Arc<Store>,
    assignment: &Assignment,
    item: &Item,
    run_id: &str,
) -> anyhow::Result<Option<LeaseOwner>> {
    let owner = LeaseOwner::new(
        store,
        &assignment.name,
        prepare::forge_name(assignment.work.forge),
        &item.external_id,
        run_id,
    )
    .context("creating lease owner")?;
    let won = owner
        .claim(bureau::supervise::LEASE_TTL)
        .context("claiming work item")?;
    if !won {
        out::line(format_args!(
            "item `{}` is already claimed",
            item.external_id
        ));
        return Ok(None);
    }
    Ok(Some(owner))
}

fn plan(
    config: &Config,
    assignment: &Assignment,
    item: Item,
    forge: Arc<dyn Forge>,
    credentials: BTreeMap<String, Secret>,
    run_id: String,
) -> RunPlan {
    RunPlan {
        run_id,
        assignment: assignment.clone(),
        pipeline: config
            .pipelines
            .get(&assignment.pipeline)
            .expect("config validation guarantees the pipeline")
            .clone(),
        roles: config.roles.clone(),
        repos: prepare::plan_repos(config, assignment),
        item,
        forge,
        credentials,
        config_source: None,
        plugin_sources: BTreeMap::new(),
        direct_agents: BTreeMap::new(),
        lease: None,
    }
}

/// The one-line outcome: `<run_id> <outcome> cost=$X.XX message [pr]`.
fn print_outcome(outcome: &RunOutcome) {
    let pr = outcome
        .pr
        .as_ref()
        .map_or(String::new(), |pr| format!(" {}", pr.url));
    out::line(format_args!(
        "{} {} cost=${:.2} {}{pr}",
        outcome.run_id,
        bureau::runlog::outcome_name(outcome.outcome),
        outcome.cost_usd,
        outcome.message
    ));
}

/// 0 for `Success`/`NoWork`, 1 otherwise.
const fn exit_code(outcome: &RunOutcome) -> i32 {
    match outcome.outcome {
        StepOutcome::Success | StepOutcome::NoWork => 0,
        StepOutcome::Failure | StepOutcome::Blocked => 1,
    }
}

async fn run_plan(plan: RunPlan, paths: &Paths, store: Arc<Store>) -> anyhow::Result<RunOutcome> {
    let _maintenance = bureau::maintenance::shared(&paths.maintenance_root)?;
    let engine = Arc::new(Engine::new(paths.runs.clone(), paths.cache.clone()));
    signal::run(engine, store, plan).await
}

/// Drives the run, then records its cost and releases the lease — the
/// release happens on every path back out.
async fn finish(plan: RunPlan, paths: &Paths, store: Arc<Store>) -> anyhow::Result<i32> {
    let outcome = run_plan(plan, paths, store).await?;
    print_outcome(&outcome);
    Ok(exit_code(&outcome))
}

/// The shared body of `run` and `retry`: resolve credentials, find the
/// item, claim it, drive the run. Every failure before the claim prints
/// its own message and maps to an exit code.
async fn execute(
    loaded: &committed::Loaded,
    assignment: &Assignment,
    item_query: &str,
    paths: &Paths,
) -> anyhow::Result<i32> {
    let Some(prepared) =
        prepare_execution(&loaded.config, assignment, &loaded.settings, item_query).await?
    else {
        return Ok(2);
    };
    let store = Arc::new(Store::open(&paths.state).context("opening state database")?);
    let run_id = new_run_id(&assignment.name).context("creating run id")?;
    let Some(owner) = claim(store.clone(), assignment, &prepared.item, &run_id)? else {
        return Ok(1);
    };
    let mut plan = plan(
        &loaded.config,
        assignment,
        prepared.item,
        prepared.forge,
        prepared.credentials,
        run_id,
    );
    plan.config_source = Some(loaded.source.clone());
    plan.direct_agents.clone_from(&loaded.direct_agents);
    plan.lease = Some(owner);
    finish(plan, paths, store).await
}

/// `run <pipeline> --item <id>`: the one-shot entry point.
///
/// # Errors
/// Propagates unexpected failures (state, forge transport, I/O).
pub async fn run(pipeline: &str, item: &str, paths: &Paths) -> anyhow::Result<i32> {
    let loaded = committed::load(&paths.settings, &paths.config_cache).await?;
    match by_pipeline(&loaded.config, pipeline) {
        Ok(assignment) => execute(&loaded, assignment, item, paths).await,
        Err(code) => Ok(code),
    }
}

/// `retry <run-id>`: a new run for the earlier run's item.
///
/// # Errors
/// Propagates unexpected failures (state, forge transport, I/O).
pub async fn retry(run_id: &str, paths: &Paths) -> anyhow::Result<i32> {
    let Some((name, item)) = retry_target(&paths.runs, run_id)? else {
        return Ok(2);
    };
    let loaded = committed::load(&paths.settings, &paths.config_cache).await?;
    let Some(assignment) = loaded.config.assignments.get(&name) else {
        out::error(format_args!(
            "assignment `{name}` from run `{run_id}` is no longer in the config"
        ));
        return Ok(2);
    };
    execute(&loaded, assignment, &item, paths).await
}
