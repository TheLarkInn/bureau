//! The command line; its hard cap is 17 top-level commands.
//!
//! `dashboard` occupies the former redundant `version` slot; use the
//! conventional `--version` flag instead.

mod command;
mod dashboard;
mod inspect;
mod lifecycle;
mod mcp;
pub mod out;
mod prepare;
mod reconcile;
mod run;
mod transcript;
mod validate;
mod watch;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use clap::Parser;

pub use command::{FakeAction, McpAction, Verb};

/// The four filesystem roots the run-side verbs work against.
struct Paths {
    /// Root holding the cross-process maintenance lock.
    maintenance_root: PathBuf,
    /// Non-secret local settings.
    settings: PathBuf,
    /// Committed config cache.
    config_cache: PathBuf,
    /// Directory holding run directories.
    runs: PathBuf,
    /// Durable state database path.
    state: PathBuf,
    /// Checkout cache directory.
    cache: PathBuf,
}

/// `bureau` — a local agent work runner.
#[derive(Debug, Parser)]
#[command(name = "bureau", version, about)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub verb: Verb,
}

fn parent(path: &std::path::Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

fn explicit_paths(
    settings: PathBuf,
    config_cache: PathBuf,
    runs: PathBuf,
    state: PathBuf,
    cache: PathBuf,
) -> Paths {
    let maintenance_root = bureau::home::Home::discover().map_or_else(
        |_| parent(&settings),
        |home| home.layout().root().to_path_buf(),
    );
    Paths {
        maintenance_root,
        settings,
        config_cache,
        runs,
        state,
        cache,
    }
}

/// Bundles the four filesystem roots a run-side verb destructures to.
fn paths(
    settings: Option<PathBuf>,
    config_cache: Option<PathBuf>,
    runs: Option<PathBuf>,
    state: Option<PathBuf>,
    cache: Option<PathBuf>,
) -> anyhow::Result<Paths> {
    let values = (settings, config_cache, runs, state, cache);
    let (settings, config_cache, runs, state, cache) = match values {
        (Some(settings), Some(config_cache), Some(runs), Some(state), Some(cache)) => {
            return Ok(explicit_paths(settings, config_cache, runs, state, cache));
        }
        values => values,
    };
    let home = bureau::home::Home::discover()?;
    let layout = home.layout();
    Ok(Paths {
        maintenance_root: layout.root().to_path_buf(),
        settings: settings.unwrap_or_else(|| layout.settings().to_path_buf()),
        config_cache: config_cache.unwrap_or_else(|| layout.config_cache().to_path_buf()),
        runs: runs.unwrap_or_else(|| layout.runs().to_path_buf()),
        state: state.unwrap_or_else(|| layout.state_db().to_path_buf()),
        cache: cache.unwrap_or_else(|| layout.checkout_cache().to_path_buf()),
    })
}

fn runs_path(path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = path {
        return Ok(path);
    }
    let home = bureau::home::Home::discover()?;
    Ok(home.layout().runs().to_path_buf())
}

async fn run_command(verb: Verb) -> anyhow::Result<i32> {
    let Verb::Run {
        pipeline,
        item,
        settings,
        config_cache,
        runs,
        state,
        cache,
    } = verb
    else {
        unreachable!("run command called with another verb")
    };
    run::run(
        &pipeline,
        &item,
        &paths(settings, config_cache, runs, state, cache)?,
    )
    .await
}

async fn retry_command(verb: Verb) -> anyhow::Result<i32> {
    let Verb::Retry {
        run_id,
        settings,
        config_cache,
        runs,
        state,
        cache,
    } = verb
    else {
        unreachable!("retry command called with another verb")
    };
    run::retry(&run_id, &paths(settings, config_cache, runs, state, cache)?).await
}

/// Shows a run's summary, its events, or both as JSON.
fn show_command(
    run_id: &str,
    events: bool,
    json: bool,
    runs: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    inspect::show(&runs_path(runs)?, run_id, events, json)
}

/// Every non-run-directory verb: the caller dispatched it already.
fn unreachable_non_run() -> i32 {
    unreachable!("handled by the caller")
}

/// Dispatches verbs that work against run directories.
async fn run_side(verb: Verb) -> anyhow::Result<i32> {
    match verb {
        Verb::Run { .. } => run_command(verb).await,
        Verb::Retry { .. } => retry_command(verb).await,
        Verb::List { runs } => Ok(inspect::list(&runs_path(runs)?)),
        Verb::Show {
            run_id,
            events,
            json,
            runs,
        } => show_command(&run_id, events, json, runs),
        Verb::Cancel { run_id, runs } => inspect::cancel(&runs_path(runs)?, &run_id),
        Verb::Pause { run_id, runs } => inspect::pause(&runs_path(runs)?, &run_id),
        Verb::Resume { run_id, runs } => inspect::resume(&runs_path(runs)?, &run_id),
        _ => Ok(unreachable_non_run()),
    }
}

type CliFuture = Pin<Box<dyn Future<Output = anyhow::Result<i32>> + Send>>;

fn dispatch(verb: Verb) -> CliFuture {
    match verb {
        Verb::Validate { dir, json } => Box::pin(async move { validate::run(&dir, json) }),
        Verb::Reconcile(args) => Box::pin(reconcile::run(args)),
        Verb::Dashboard(args) => Box::pin(async move { dashboard::run(&args) }),
        Verb::Watch {
            runs,
            state,
            config_cache,
        } => Box::pin(async move { watch::run(runs, state, config_cache) }),
        Verb::Init { from } => Box::pin(async move { lifecycle::init(&from).await }),
        Verb::Setup { from } => Box::pin(async move { lifecycle::setup(&from).await }),
        Verb::Doctor { json } => Box::pin(async move { lifecycle::doctor(json) }),
        Verb::Repair {
            clear_checkout_cache,
            clear_config_cache,
        } => Box::pin(async move { lifecycle::repair(clear_checkout_cache, clear_config_cache) }),
        Verb::Mcp { action } => Box::pin(async move { mcp::run(&action) }),
        Verb::Fake { action } => Box::pin(transcript::run(action)),
        verb => Box::pin(run_side(verb)),
    }
}

/// Runs the CLI and returns the process exit code.
///
/// # Errors
/// Propagates unexpected failures (fixture I/O, serialization).
pub async fn run(cli: Cli) -> anyhow::Result<i32> {
    dispatch(cli.verb).await
}
