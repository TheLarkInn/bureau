use std::path::PathBuf;

use super::args::NetworkArgs;

#[derive(Debug, clap::Args)]
#[group(id = "cloud_run_mode", multiple = false)]
pub struct RunMode {
    /// Submit once to an existing automation; no returned task ID is assumed.
    #[arg(long, requires = "github_cloud", conflicts_with = "automation")]
    pub dispatch_automation: Option<String>,
    /// Record an exact operator-selected task; never dispatch a new task.
    #[arg(long, requires_all = ["github_cloud", "automation"])]
    pub track_task: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct RunArgs {
    /// Pipeline name from the config repo.
    #[arg(
        required_unless_present = "github_cloud",
        conflicts_with = "github_cloud"
    )]
    pub pipeline: Option<String>,
    /// Work item ID on the assignment's forge.
    #[arg(
        long,
        required_unless_present = "github_cloud",
        conflicts_with = "github_cloud"
    )]
    pub item: Option<String>,
    /// Experimental cloud controls, not a Bureau pipeline or remote steering.
    #[arg(long, requires_all = ["repo", "expected_login", "request_id", "cloud_run_mode"])]
    pub github_cloud: bool,
    #[command(flatten)]
    pub mode: RunMode,
    /// Automation owning the explicitly selected task.
    #[arg(long, requires_all = ["github_cloud", "track_task"])]
    pub automation: Option<String>,
    /// Stable local receipt key; reusing it never submits a second task.
    #[arg(long, requires = "github_cloud")]
    pub request_id: Option<String>,
    #[command(flatten)]
    pub network: NetworkArgs,
    /// Directory holding pipeline runs or cloud receipts in cloud mode.
    #[arg(long)]
    pub runs: Option<PathBuf>,
    /// Durable state database path.
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// Checkout cache directory for pipeline execution.
    #[arg(long, conflicts_with = "github_cloud")]
    pub cache: Option<PathBuf>,
    /// Emits structured cloud receipt JSON.
    #[arg(long, requires = "github_cloud")]
    pub json: bool,
}
