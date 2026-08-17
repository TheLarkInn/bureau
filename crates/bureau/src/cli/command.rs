//! Clap command schema, separate from command execution.

use std::path::PathBuf;

use clap::Subcommand;

use super::reconcile;

/// Fake adapter operations.
#[derive(Debug, Subcommand)]
pub enum FakeAction {
    /// Replays a transcript fixture.
    Replay {
        /// Fixture path.
        fixture: PathBuf,
    },
    /// Records a command.
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
        /// Local settings file override.
        #[arg(long)]
        settings: Option<PathBuf>,
        /// Committed config cache override.
        #[arg(long)]
        config_cache: Option<PathBuf>,
        /// Directory holding run directories.
        #[arg(long)]
        runs: Option<PathBuf>,
        /// Durable state database path.
        #[arg(long)]
        state: Option<PathBuf>,
        /// Checkout cache directory.
        #[arg(long)]
        cache: Option<PathBuf>,
    },
    /// Lists runs.
    List {
        /// Directory holding run directories.
        #[arg(long)]
        runs: Option<PathBuf>,
    },
    /// Shows one run's replayed state.
    Show {
        /// The run id.
        run_id: String,
        /// Directory holding run directories.
        #[arg(long)]
        runs: Option<PathBuf>,
    },
    /// Cancels a running run by writing its CANCEL marker.
    Cancel {
        /// The run id.
        run_id: String,
        /// Directory holding run directories.
        #[arg(long)]
        runs: Option<PathBuf>,
    },
    /// Starts a new run for the item an earlier run targeted.
    Retry {
        /// The earlier run id.
        run_id: String,
        /// Local settings file override.
        #[arg(long)]
        settings: Option<PathBuf>,
        /// Committed config cache override.
        #[arg(long)]
        config_cache: Option<PathBuf>,
        /// Directory holding run directories.
        #[arg(long)]
        runs: Option<PathBuf>,
        /// Durable state database path.
        #[arg(long)]
        state: Option<PathBuf>,
        /// Checkout cache directory.
        #[arg(long)]
        cache: Option<PathBuf>,
    },
    /// Continuously reconciles committed config with forge state.
    Reconcile(reconcile::Args),
    /// Performs first-time local initialization.
    Init {
        /// YAML initialization request.
        #[arg(long)]
        from: PathBuf,
    },
    /// Replaces non-secret local settings.
    Setup {
        /// YAML settings file to adopt.
        #[arg(long)]
        from: PathBuf,
    },
    /// Runs read-only offline diagnostics.
    Doctor {
        /// Emits structured JSON.
        #[arg(long)]
        json: bool,
    },
    /// Applies explicitly confirmed reversible repairs.
    Repair {
        /// Requests clearing the disposable checkout cache.
        #[arg(long)]
        clear_checkout_cache: bool,
        /// Requests clearing the disposable config cache.
        #[arg(long)]
        clear_config_cache: bool,
    },
    /// Serves the adapter step I/O protocol.
    #[command(hide = true)]
    Mcp {
        /// MCP operation.
        #[command(subcommand)]
        action: McpAction,
    },
    /// Replays or records adapter transcripts.
    Fake {
        /// What to do with the fixture.
        #[command(subcommand)]
        action: FakeAction,
    },
}
