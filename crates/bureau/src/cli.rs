//! The command line; verbs are verbs and the hard cap is 15.

mod inspect;
mod mcp;
mod prepare;
mod reconcile;
mod run;
mod transcript;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use clap::{Parser, Subcommand};

use bureau::config::Config;
use bureau::contract::StepOutcome;

/// The four filesystem roots the run-side verbs work against.
struct Paths {
    /// Config repository checkout.
    config: PathBuf,
    /// Directory holding run directories.
    runs: PathBuf,
    /// Durable state database path.
    state: PathBuf,
    /// Checkout cache directory.
    cache: PathBuf,
}

/// The kebab-case token for an outcome (its serde name).
const fn outcome_name(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Success => "success",
        StepOutcome::Failure => "failure",
        StepOutcome::Blocked => "blocked",
        StepOutcome::NoWork => "no-work",
    }
}

/// `bureau` — a local agent work runner.
#[derive(Debug, Parser)]
#[command(name = "bureau", version, about)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub verb: Verb,
}

/// CLI verbs.
#[derive(Debug, Subcommand)]
pub enum Verb {
    /// Checks a config directory and reports every error in one pass.
    Validate {
        /// Path to the config repository checkout.
        #[arg(default_value = "runner-config")]
        dir: PathBuf,
    },
    /// Prints the version.
    Version,
    /// Runs a pipeline once for one work item.
    Run {
        /// Pipeline name from the config repo.
        pipeline: String,
        /// Work item id on the assignment's forge.
        #[arg(long)]
        item: String,
        /// Path to the config repository checkout.
        #[arg(long, default_value = "runner-config")]
        config: PathBuf,
        /// Directory holding run directories.
        #[arg(long, default_value = "runs")]
        runs: PathBuf,
        /// Durable state database path.
        #[arg(long, default_value = "state.db")]
        state: PathBuf,
        /// Checkout cache directory.
        #[arg(long, default_value = "checkout-cache")]
        cache: PathBuf,
    },
    /// Lists runs.
    List {
        /// Directory holding run directories.
        #[arg(long, default_value = "runs")]
        runs: PathBuf,
    },
    /// Shows one run's replayed state.
    Show {
        /// The run id.
        run_id: String,
        /// Directory holding run directories.
        #[arg(long, default_value = "runs")]
        runs: PathBuf,
    },
    /// Cancels a running run by writing its CANCEL marker.
    Cancel {
        /// The run id.
        run_id: String,
        /// Directory holding run directories.
        #[arg(long, default_value = "runs")]
        runs: PathBuf,
    },
    /// Starts a new run for the item an earlier run targeted.
    Retry {
        /// The earlier run id.
        run_id: String,
        /// Path to the config repository checkout.
        #[arg(long, default_value = "runner-config")]
        config: PathBuf,
        /// Directory holding run directories.
        #[arg(long, default_value = "runs")]
        runs: PathBuf,
        /// Durable state database path.
        #[arg(long, default_value = "state.db")]
        state: PathBuf,
        /// Checkout cache directory.
        #[arg(long, default_value = "checkout-cache")]
        cache: PathBuf,
    },
    /// Continuously reconciles committed config with forge state.
    Reconcile(reconcile::Args),
    /// Serves the adapter step I/O protocol.
    #[command(hide = true)]
    Mcp {
        /// MCP operation.
        #[command(subcommand)]
        action: McpAction,
    },
    /// Replays or records adapter transcripts — the testing seam.
    Fake {
        /// What to do with the fixture.
        #[command(subcommand)]
        action: FakeAction,
    },
}

/// `fake` adapter operations.
#[derive(Debug, Subcommand)]
pub enum FakeAction {
    /// Replays a transcript fixture with its recorded exit code.
    Replay {
        /// Fixture path.
        fixture: PathBuf,
    },
    /// Records a command under the layer-0 contract.
    Record {
        /// Fixture path to write.
        fixture: PathBuf,
        /// The command to run, after `--`.
        #[arg(last = true, required = true)]
        argv: Vec<String>,
    },
}

/// MCP protocol operations.
#[derive(Debug, Subcommand)]
pub enum McpAction {
    /// Serves MCP over standard input and output.
    #[command(hide = true)]
    Serve,
}

/// Runs the CLI and returns the process exit code.
///
/// # Errors
/// Propagates unexpected failures (fixture I/O, serialization).
pub async fn run(cli: Cli) -> anyhow::Result<i32> {
    dispatch(cli.verb).await
}

type CliFuture = Pin<Box<dyn Future<Output = anyhow::Result<i32>> + Send>>;

fn dispatch(verb: Verb) -> CliFuture {
    match verb {
        Verb::Version => Box::pin(async { Ok(version()) }),
        Verb::Validate { dir } => Box::pin(async move { Ok(validate(&dir)) }),
        Verb::Reconcile(args) => Box::pin(reconcile::run(args)),
        Verb::Mcp { action } => Box::pin(async move { mcp::run(&action) }),
        Verb::Fake { action } => Box::pin(transcript::run(action)),
        verb => Box::pin(run_side(verb)),
    }
}

/// Dispatches verbs that work against run directories.
async fn run_side(verb: Verb) -> anyhow::Result<i32> {
    match verb {
        Verb::Run { .. } => run_command(verb).await,
        Verb::Retry { .. } => retry_command(verb).await,
        Verb::List { runs } => Ok(inspect::list(&runs)),
        Verb::Show { run_id, runs } => inspect::show(&runs, &run_id),
        Verb::Cancel { run_id, runs } => inspect::cancel(&runs, &run_id),
        Verb::Version
        | Verb::Validate { .. }
        | Verb::Reconcile(_)
        | Verb::Mcp { .. }
        | Verb::Fake { .. } => {
            unreachable!("handled by the caller")
        }
    }
}

async fn run_command(verb: Verb) -> anyhow::Result<i32> {
    let Verb::Run {
        pipeline,
        item,
        config,
        runs,
        state,
        cache,
    } = verb
    else {
        unreachable!("run command called with another verb")
    };
    run::run(&pipeline, &item, &paths(config, runs, state, cache)).await
}

async fn retry_command(verb: Verb) -> anyhow::Result<i32> {
    let Verb::Retry {
        run_id,
        config,
        runs,
        state,
        cache,
    } = verb
    else {
        unreachable!("retry command called with another verb")
    };
    run::retry(&run_id, &paths(config, runs, state, cache)).await
}

/// Bundles the four filesystem roots a run-side verb destructures to.
const fn paths(config: PathBuf, runs: PathBuf, state: PathBuf, cache: PathBuf) -> Paths {
    Paths {
        config,
        runs,
        state,
        cache,
    }
}

/// Prints the version line.
fn version() -> i32 {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    0
}

fn validate(dir: &std::path::Path) -> i32 {
    match Config::load(dir) {
        Ok(config) => {
            println!(
                "config ok: {} repos, {} roles, {} assignments",
                config.repos.len(),
                config.roles.len(),
                config.assignments.len()
            );
            0
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("{error}");
            }
            eprintln!("{} config error(s)", errors.len());
            1
        }
    }
}
