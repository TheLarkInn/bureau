//! `run` and `retry`: claim one work item and drive one pipeline run to
//! a terminal (DESIGN.md sections 11 and 13).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;

use bureau::config::{Assignment, Config};
use bureau::contract::StepOutcome;
use bureau::engine::{Engine, RunOutcome, RunPlan, new_run_id};
use bureau::forge::{Forge, Item};
use bureau::process::Secret;
use bureau::runlog::{self, EventKind, RunStartedData};
use bureau::state::Store;

use super::{Line, Paths, prepare};

/// A claimed item's lease lasts 30 minutes; expiry is the crash release
/// (DESIGN.md layer 5).
const LEASE_TTL: Duration = Duration::from_secs(30 * 60);

/// Loads the config, collecting every error line and yielding `None` on any.
fn load_config(dir: &Path, lines: &mut Vec<Line>) -> Option<Config> {
    match Config::load(dir) {
        Ok(config) => Some(config),
        Err(errors) => {
            for error in &errors {
                lines.push(Line::Err(error.to_string()));
            }
            lines.push(Line::Err(format!("{} config error(s)", errors.len())));
            None
        }
    }
}

/// The one assignment bound to `pipeline`; v0 runs exactly one, so zero
/// matches and ambiguity are both errors that name what was found.
fn by_pipeline<'c>(
    config: &'c Config,
    pipeline: &str,
    lines: &mut Vec<Line>,
) -> Result<&'c Assignment, i32> {
    let found: Vec<&Assignment> = config
        .assignments
        .values()
        .filter(|a| a.pipeline == pipeline)
        .collect();
    match found.as_slice() {
        [] => {
            lines.push(Line::Err(format!(
                "no assignment uses pipeline `{pipeline}`"
            )));
            Err(2)
        }
        [one] => Ok(one),
        many => {
            let names = many.iter().map(|a| a.name.as_str()).collect::<Vec<_>>();
            let count = many.len();
            lines.push(Line::Err(format!(
                "pipeline `{pipeline}` is used by {count} assignments: {}",
                names.join(", ")
            )));
            Err(2)
        }
    }
}

/// What `retry` re-runs: the original run's assignment name and item,
/// read from its `run_started` event.
fn retry_target(
    runs: &Path,
    run_id: &str,
    lines: &mut Vec<Line>,
) -> anyhow::Result<Option<(String, String)>> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        lines.push(Line::Err(format!("no such run: `{run_id}`")));
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
    lines.push(Line::Err(format!(
        "run `{run_id}` recorded no work item; nothing to retry"
    )));
    Ok(None)
}

/// Assembles the run's plan; the pipeline lookup cannot fail (config
/// validation guarantees it).
fn plan(
    config: &Config,
    assignment: &Assignment,
    item: Item,
    forge: Arc<dyn Forge>,
    credentials: BTreeMap<String, Secret>,
    env: &BTreeMap<String, String>,
    clock: fn() -> u64,
) -> RunPlan {
    RunPlan {
        run_id: new_run_id(&assignment.name, clock()),
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
        daemon_env: env.clone(),
    }
}

/// Builds the plan and runs the engine (which never panics across its
/// boundary).
async fn drive(plan: RunPlan, paths: &Paths, clock: fn() -> u64) -> RunOutcome {
    Engine::new(paths.runs.clone(), paths.cache.clone(), clock)
        .run(&plan)
        .await
}

/// The one-line outcome: `<run_id> <outcome> cost=$X.XX message [pr]`.
fn outcome_line(outcome: &RunOutcome) -> String {
    let pr = outcome
        .pr
        .as_ref()
        .map_or(String::new(), |pr| format!(" {}", pr.url));
    format!(
        "{} {} cost=${:.2} {}{pr}",
        outcome.run_id,
        super::outcome_name(outcome.outcome),
        outcome.cost_usd,
        outcome.message
    )
}

/// 0 for `Success`/`NoWork`, 1 otherwise.
const fn exit_code(outcome: &RunOutcome) -> i32 {
    match outcome.outcome {
        StepOutcome::Success | StepOutcome::NoWork => 0,
        StepOutcome::Failure | StepOutcome::Blocked => 1,
    }
}

/// Claims the item for 30 minutes; on loss, says so and exits 1.
fn claim(
    store: &Store,
    assignment: &Assignment,
    item: &Item,
    lines: &mut Vec<Line>,
) -> anyhow::Result<bool> {
    let won = store
        .try_claim(
            &assignment.name,
            prepare::forge_name(assignment.work.forge),
            &item.external_id,
            LEASE_TTL,
        )
        .context("claiming work item")?;
    if !won {
        lines.push(Line::Out(format!(
            "item `{}` is already claimed",
            item.external_id
        )));
    }
    Ok(won)
}

/// Drives the run, then records its cost and releases the lease — the
/// release happens on every path back out.
async fn finish(
    plan: RunPlan,
    paths: &Paths,
    store: &Store,
    clock: fn() -> u64,
    lines: &mut Vec<Line>,
) -> anyhow::Result<i32> {
    let (name, external_id) = (plan.assignment.name.clone(), plan.item.external_id.clone());
    let outcome = drive(plan, paths, clock).await;
    let recorded = store
        .record_run(&name, outcome.cost_usd)
        .context("recording run cost");
    let released = store
        .release(&name, &external_id)
        .context("releasing lease");
    recorded.and(released)?;
    lines.push(Line::Out(outcome_line(&outcome)));
    Ok(exit_code(&outcome))
}

/// The shared body of `run` and `retry`: resolve credentials, find the
/// item, claim it, drive the run. Every failure before the claim
/// collects its own message and maps to an exit code.
async fn execute(
    config: &Config,
    assignment: &Assignment,
    item_query: &str,
    paths: &Paths,
    env: &BTreeMap<String, String>,
    clock: fn() -> u64,
    lines: &mut Vec<Line>,
) -> anyhow::Result<i32> {
    let Some(credentials) = prepare::resolve_credentials(config, assignment, env, lines) else {
        return Ok(2);
    };
    let forge = prepare::work_forge(config, assignment, &credentials)?;
    let Some(item) = prepare::find_item(&*forge, assignment, item_query).await? else {
        let source = &assignment.work.source;
        lines.push(Line::Err(format!("no item `{item_query}` in `{source}`")));
        return Ok(2);
    };
    let store = Store::open(&paths.state, clock).context("opening state database")?;
    if !claim(&store, assignment, &item, lines)? {
        return Ok(1);
    }
    let plan = plan(config, assignment, item, forge, credentials, env, clock);
    finish(plan, paths, &store, clock, lines).await
}

/// `run <pipeline> --item <id>`: the one-shot entry point.
///
/// # Errors
/// Propagates unexpected failures (state, forge transport, I/O).
pub async fn run(
    pipeline: &str,
    item: &str,
    paths: &Paths,
    env: BTreeMap<String, String>,
    clock: fn() -> u64,
    lines: &mut Vec<Line>,
) -> anyhow::Result<i32> {
    let Some(config) = load_config(&paths.config, lines) else {
        return Ok(2);
    };
    match by_pipeline(&config, pipeline, lines) {
        Ok(assignment) => execute(&config, assignment, item, paths, &env, clock, lines).await,
        Err(code) => Ok(code),
    }
}

/// `retry <run-id>`: a new run for the earlier run's item.
///
/// # Errors
/// Propagates unexpected failures (state, forge transport, I/O).
pub async fn retry(
    run_id: &str,
    paths: &Paths,
    env: BTreeMap<String, String>,
    clock: fn() -> u64,
    lines: &mut Vec<Line>,
) -> anyhow::Result<i32> {
    let Some((name, item)) = retry_target(&paths.runs, run_id, lines)? else {
        return Ok(2);
    };
    let Some(config) = load_config(&paths.config, lines) else {
        return Ok(2);
    };
    let Some(assignment) = config.assignments.get(&name) else {
        lines.push(Line::Err(format!(
            "assignment `{name}` from run `{run_id}` is no longer in the config"
        )));
        return Ok(2);
    };
    execute(&config, assignment, &item, paths, &env, clock, lines).await
}
