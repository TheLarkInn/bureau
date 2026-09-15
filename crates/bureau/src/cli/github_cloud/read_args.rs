use std::path::PathBuf;

use super::args::NetworkArgs;

#[derive(Debug, clap::Args)]
pub struct ListArgs {
    /// Directory holding local pipeline runs.
    #[arg(long, conflicts_with = "github_cloud")]
    pub runs: Option<PathBuf>,
    /// List experimental cloud automations or their task history.
    #[arg(long, requires_all = ["repo", "expected_login"])]
    pub github_cloud: bool,
    /// List this automation's task history instead of automations.
    #[arg(long, requires = "github_cloud")]
    pub automation: Option<String>,
    #[command(flatten)]
    pub network: NetworkArgs,
    /// Emits structured cloud inventory JSON.
    #[arg(long, requires = "github_cloud")]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct EventOutputArgs {
    /// Shows events instead of the state summary.
    #[arg(long)]
    pub events: bool,
    /// Emits structured state JSON, or the raw event array with --events.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct ShowArgs {
    /// Pipeline run ID, or a cloud receipt key in cloud mode.
    #[arg(required_unless_present = "automation", conflicts_with = "automation")]
    pub run_id: Option<String>,
    #[command(flatten)]
    pub output: EventOutputArgs,
    /// Directory holding pipeline runs or cloud receipts in cloud mode.
    #[arg(long)]
    pub runs: Option<PathBuf>,
    /// Inspect experimental cloud definitions or local cloud receipts.
    #[arg(long)]
    pub github_cloud: bool,
    /// Inspect a remote definition rather than a receipt.
    #[arg(long, requires_all = ["github_cloud", "repo", "expected_login"], conflicts_with_all = ["events", "refresh", "runs", "state"])]
    pub automation: Option<String>,
    /// Fetch and record current exact-task observations; never resume execution.
    #[arg(long, requires_all = ["github_cloud", "run_id"])]
    pub refresh: bool,
    #[command(flatten)]
    pub network: NetworkArgs,
    /// Database used to fence a cloud observation refresh.
    #[arg(long, requires = "refresh")]
    pub state: Option<PathBuf>,
}
