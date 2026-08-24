//! Local lifecycle command adapters.
mod doctor;
mod init;
mod migrate;
mod repair;
mod setup;

use std::path::Path;

pub(super) async fn setup(from: &Path) -> anyhow::Result<i32> {
    setup::run(from).await
}

pub(super) async fn init(from: &Path) -> anyhow::Result<i32> {
    init::run(from).await
}

pub(super) async fn doctor(json: bool) -> anyhow::Result<i32> {
    doctor::run(json).await
}

pub(super) fn repair(clear_checkout_cache: bool, clear_config_cache: bool) -> anyhow::Result<i32> {
    repair::run(clear_checkout_cache, clear_config_cache)
}
