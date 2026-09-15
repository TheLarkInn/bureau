//! Clap command schema, separate from command execution.

use std::path::PathBuf;

use clap::Subcommand;

use super::github_cloud::{ControlArgs, ListArgs, RunArgs, ShowArgs};
use super::{dashboard, reconcile};

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

/// CLI commands.
#[derive(Debug, Subcommand)]
pub enum Verb {
    /// Checks a config directory and reports every error in one pass.
    Validate {
        /// Path to the config repository checkout.
        #[arg(default_value = "runner-config")]
        dir: PathBuf,
        /// Emits structured JSON.
        #[arg(long)]
        json: bool,
    },
    /// Runs a pipeline, or explicitly submits/tracks a cloud task.
    Run(RunArgs),
    /// Lists local runs, or experimental cloud inventory.
    List(ListArgs),
    /// Shows local run state or explicit cloud observations.
    Show(ShowArgs),
    /// Cancels a running run by writing its CANCEL marker.
    Cancel(ControlArgs),
    /// Pauses a running run at its next step boundary by writing its
    /// PAUSE marker.
    Pause(ControlArgs),
    /// Clears a run's PAUSE marker so the next `bureau run` re-entry
    /// (or the reconcile loop) resumes it; this verb does not itself
    /// continue the run.
    Resume(ControlArgs),
    /// Starts a new run for the item an earlier run targeted.
    Retry {
        /// The earlier run id.
        run_id: String,
        /// Rejects unsupported remote retries without issuing a request.
        #[arg(long)]
        github_cloud: bool,
        /// Emits the unsupported cloud control as JSON.
        #[arg(long, requires = "github_cloud")]
        json: bool,
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
    /// Opens the pipeline drafting table and run dashboard in a browser.
    Dashboard(dashboard::DashboardArgs),
    /// Watches local state in a live terminal dashboard; piped, prints
    /// one snapshot.
    Watch {
        /// Directory holding run directories.
        #[arg(long)]
        runs: Option<PathBuf>,
        /// Durable state database path.
        #[arg(long)]
        state: Option<PathBuf>,
        /// Committed config cache directory.
        #[arg(long)]
        config_cache: Option<PathBuf>,
    },
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
