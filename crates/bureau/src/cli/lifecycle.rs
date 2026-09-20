//! Local lifecycle command adapters.
mod doctor;
mod init;
mod migrate;
mod repair;
mod setup;

use std::path::{Path, PathBuf};

use anyhow::Context as _;

/// First-time initialization or print-only request authoring.
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
pub struct InitArgs {
    /// YAML initialization request; may install plugins, open a PR and run merged work.
    #[arg(long)]
    from: Option<PathBuf>,
    /// Prints an editable request to stdout without reading settings or starting work.
    #[arg(long)]
    print_template: bool,
}

pub(super) async fn setup(from: &Path) -> anyhow::Result<i32> {
    setup::run(from).await
}

pub(super) async fn init(args: &InitArgs) -> anyhow::Result<i32> {
    if args.print_template {
        init::print_template();
        return Ok(0);
    }
    let from = args
        .from
        .as_deref()
        .context("--from is required unless --print-template is used")?;
    init::run(from).await
}

pub(super) fn doctor(json: bool) -> anyhow::Result<i32> {
    doctor::run(json)
}

pub(super) fn repair(clear_checkout_cache: bool, clear_config_cache: bool) -> anyhow::Result<i32> {
    repair::run(clear_checkout_cache, clear_config_cache)
}
