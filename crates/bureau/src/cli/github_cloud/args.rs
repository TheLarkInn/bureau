use std::path::PathBuf;

#[derive(Debug, Default, clap::Args)]
pub struct NetworkArgs {
    /// Committed registry name of the dotcom repository.
    #[arg(long, requires = "github_cloud")]
    pub repo: Option<String>,
    /// Expected GitHub user; cloud entitlement remains experimental.
    #[arg(long, requires = "github_cloud")]
    pub expected_login: Option<String>,
    /// Local settings file override.
    #[arg(long)]
    pub settings: Option<PathBuf>,
    /// Committed config cache override.
    #[arg(long)]
    pub config_cache: Option<PathBuf>,
}

impl NetworkArgs {
    pub const fn has_options(&self) -> bool {
        self.repo.is_some()
            || self.expected_login.is_some()
            || self.settings.is_some()
            || self.config_cache.is_some()
    }
}

#[derive(Debug, clap::Args)]
pub struct ControlArgs {
    /// Local pipeline run ID, or cloud receipt key with --github-cloud.
    pub run_id: String,
    /// Directory holding records.
    #[arg(long)]
    pub runs: Option<PathBuf>,
    /// Reject unavailable remote controls instead of changing pipeline markers.
    #[arg(long)]
    pub github_cloud: bool,
    /// Emits the unsupported cloud action as JSON.
    #[arg(long, requires = "github_cloud")]
    pub json: bool,
}
