use std::collections::BTreeSet;

use bureau::runlog::RunStatus;
use bureau::setup::{Outcome, OutcomeSummary, RunSummary, Settings, ValidatedConfig};

use crate::cli::reconcile::{self, Args, ForgeArg};

use super::access;
use super::files::Temporary;

pub(super) async fn run(
    layout: &bureau::home::Layout,
    settings: &Settings,
    config: &ValidatedConfig,
) -> anyhow::Result<OutcomeSummary> {
    let before = run_ids(layout)?;
    let staging = Temporary::new(layout.config_cache(), "init-reconcile")?;
    let settings_path = staging.path().join("settings.yaml");
    bureau::setup::save_settings(&settings_path, settings)?;
    reconcile::run(arguments(layout, settings, config, settings_path)).await?;
    summaries(layout, &before)
}

fn arguments(
    layout: &bureau::home::Layout,
    settings: &Settings,
    config: &ValidatedConfig,
    settings_path: std::path::PathBuf,
) -> Args {
    Args {
        maintenance_root: Some(layout.root().to_path_buf()),
        settings: Some(settings_path),
        config_remote: Some(config.source.remote().to_owned()),
        config_ref: Some(config.commit.clone()),
        config_subdir: Some(config.source.subdirectory().to_path_buf()),
        config_credential: settings
            .credentials
            .contains_key("config")
            .then(|| "config".into()),
        config_forge: forge_arg(settings),
        config_cache: Some(layout.config_cache().to_path_buf()),
        runs: Some(layout.runs().to_path_buf()),
        state: Some(layout.state_db().to_path_buf()),
        cache: Some(layout.checkout_cache().to_path_buf()),
        interval: "5m".to_owned(),
        now: true,
    }
}

fn forge_arg(settings: &Settings) -> ForgeArg {
    match access::forge_kind(settings.config.remote()) {
        bureau::config::ForgeKind::Github => ForgeArg::Github,
        bureau::config::ForgeKind::Ado => ForgeArg::Ado,
    }
}

fn summaries(
    layout: &bureau::home::Layout,
    before: &BTreeSet<String>,
) -> anyhow::Result<OutcomeSummary> {
    let mut runs = Vec::new();
    for id in run_ids(layout)?.difference(before) {
        let state = bureau::runlog::replay_state(&layout.runs().join(id))?;
        runs.push(RunSummary {
            run_id: id.clone(),
            outcome: outcome(&state.status)?,
        });
    }
    Ok(OutcomeSummary { runs })
}

fn run_ids(layout: &bureau::home::Layout) -> anyhow::Result<BTreeSet<String>> {
    let entries = match std::fs::read_dir(layout.runs()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
    };
    Ok(entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect())
}

fn outcome(status: &RunStatus) -> anyhow::Result<Outcome> {
    let RunStatus::Finished(outcome) = status else {
        anyhow::bail!("reconcile returned before a run finished");
    };
    Ok(match *outcome {
        bureau::contract::StepOutcome::Success => Outcome::Success,
        bureau::contract::StepOutcome::Failure => Outcome::Failure,
        bureau::contract::StepOutcome::Blocked => Outcome::Blocked,
        bureau::contract::StepOutcome::NoWork => Outcome::NoWork,
    })
}
