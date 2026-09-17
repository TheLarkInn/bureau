//! The command line; its hard cap is 17 top-level commands.
//!
//! `dashboard` occupies the former redundant `version` slot; use the
//! conventional `--version` flag instead.

mod command;
mod dashboard;
mod factory_credentials;
mod github_cloud;
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

async fn run_command(args: github_cloud::RunArgs) -> anyhow::Result<i32> {
    if args.github_cloud {
        return github_cloud::run(&args).await;
    }
    let pipeline = args
        .pipeline
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("pipeline is required"))?;
    let item = args
        .item
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--item is required"))?;
    let paths = paths(
        args.network.settings,
        args.network.config_cache,
        args.runs,
        args.state,
        args.cache,
    )?;
    run::run(pipeline, item, &paths).await
}

async fn retry_command(verb: Verb) -> anyhow::Result<i32> {
    let Verb::Retry {
        run_id,
        github_cloud,
        json,
        settings,
        config_cache,
        runs,
        state,
        cache,
    } = verb
    else {
        unreachable!("retry command called with another verb")
    };
    if github_cloud {
        return github_cloud::unsupported(json);
    }
    run::retry(&run_id, &paths(settings, config_cache, runs, state, cache)?).await
}

async fn show_command(args: github_cloud::ShowArgs) -> anyhow::Result<i32> {
    if args.github_cloud {
        return github_cloud::show(&args).await;
    }
    anyhow::ensure!(
        !args.network.has_options(),
        "network options require --github-cloud"
    );
    let run_id = args
        .run_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("run ID is required"))?;
    inspect::show(
        &runs_path(args.runs)?,
        run_id,
        args.output.events,
        args.output.json,
    )
}

async fn list_command(args: github_cloud::ListArgs) -> anyhow::Result<i32> {
    if args.github_cloud {
        return github_cloud::list(&args).await;
    }
    anyhow::ensure!(
        !args.network.has_options(),
        "network options require --github-cloud"
    );
    Ok(inspect::list(&runs_path(args.runs)?))
}

fn control_command(
    args: github_cloud::ControlArgs,
    action: fn(&std::path::Path, &str) -> anyhow::Result<i32>,
) -> anyhow::Result<i32> {
    if args.github_cloud {
        return github_cloud::unsupported(args.json);
    }
    action(&runs_path(args.runs)?, &args.run_id)
}

/// Every non-run-directory verb: the caller dispatched it already.
fn unreachable_non_run() -> i32 {
    unreachable!("handled by the caller")
}

type CliFuture = Pin<Box<dyn Future<Output = anyhow::Result<i32>> + Send>>;

/// Dispatches verbs that work against run directories.
fn run_side(verb: Verb) -> CliFuture {
    match verb {
        Verb::Run(args) => Box::pin(run_command(args)),
        Verb::Retry { .. } => Box::pin(retry_command(verb)),
        Verb::List(args) => Box::pin(list_command(args)),
        Verb::Show(args) => Box::pin(show_command(args)),
        Verb::Cancel(args) => Box::pin(async move { control_command(args, inspect::cancel) }),
        Verb::Pause(args) => Box::pin(async move { control_command(args, inspect::pause) }),
        Verb::Resume(args) => Box::pin(async move { control_command(args, inspect::resume) }),
        _ => Box::pin(async { Ok(unreachable_non_run()) }),
    }
}

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
        Verb::Init(args) => Box::pin(async move { lifecycle::init(&args).await }),
        Verb::Setup { from } => Box::pin(async move { lifecycle::setup(&from).await }),
        Verb::Doctor { json } => Box::pin(async move { lifecycle::doctor(json) }),
        Verb::Repair {
            clear_checkout_cache,
            clear_config_cache,
        } => Box::pin(async move { lifecycle::repair(clear_checkout_cache, clear_config_cache) }),
        Verb::Mcp { action } => Box::pin(async move { mcp::run(&action) }),
        Verb::Fake { action } => Box::pin(transcript::run(action)),
        verb => run_side(verb),
    }
}

/// Runs the CLI and returns the process exit code.
///
/// # Errors
/// Propagates unexpected failures (fixture I/O, serialization).
pub async fn run(cli: Cli) -> anyhow::Result<i32> {
    dispatch(cli.verb).await
}
