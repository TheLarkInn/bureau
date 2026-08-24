use std::collections::BTreeMap;

use bureau::runlog::RunStatus;
use bureau::setup::{Outcome, OutcomeSummary, RunSummary, Settings, ValidatedConfig};

use crate::cli::reconcile::{self, Args};

use super::super::migrate;
use super::files::Temporary;

struct DurablePaths {
    runs: std::path::PathBuf,
    state: std::path::PathBuf,
}

fn durable_paths(
    layout: &bureau::home::Layout,
    migration: Option<&migrate::Prepared>,
) -> DurablePaths {
    migration.map_or_else(
        || DurablePaths {
            runs: layout.runs().to_path_buf(),
            state: layout.state_db().to_path_buf(),
        },
        |migration| DurablePaths {
            runs: migration.runs_path(),
            state: migration.state_path(),
        },
    )
}

fn arguments(
    layout: &bureau::home::Layout,
    settings: &Settings,
    config: &ValidatedConfig,
    settings_path: std::path::PathBuf,
    durable: &DurablePaths,
) -> Args {
    Args {
        maintenance_guarded: true,
        maintenance_root: Some(layout.root().to_path_buf()),
        settings: Some(settings_path),
        config_remote: Some(config.source.remote().to_owned()),
        config_ref: Some(config.commit.clone()),
        config_subdir: Some(config.source.subdirectory().to_path_buf()),
        config_credential: settings
            .credentials
            .contains_key(bureau::config::CONFIG_CREDENTIAL)
            .then(|| bureau::config::CONFIG_CREDENTIAL.into()),
        config_forge: None,
        config_cache: Some(layout.config_cache().to_path_buf()),
        runs: Some(durable.runs.clone()),
        state: Some(durable.state.clone()),
        cache: Some(layout.checkout_cache().to_path_buf()),
        interval: "5m".to_owned(),
        now: true,
    }
}

fn run_ids(runs_root: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let entries = match std::fs::read_dir(runs_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
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

fn run_states(runs_root: &std::path::Path) -> anyhow::Result<BTreeMap<String, RunStatus>> {
    run_ids(runs_root)?
        .into_iter()
        .map(|id| {
            let state = bureau::runlog::replay_state(&runs_root.join(&id))?;
            Ok((id, state.status))
        })
        .collect()
}

fn summaries(
    runs_root: &std::path::Path,
    before: &BTreeMap<String, RunStatus>,
) -> anyhow::Result<OutcomeSummary> {
    let mut runs = Vec::new();
    for id in run_ids(runs_root)? {
        let state = bureau::runlog::replay_state(&runs_root.join(&id))?;
        if before
            .get(&id)
            .is_some_and(|status| status != &RunStatus::Running)
        {
            continue;
        }
        runs.push(RunSummary {
            run_id: id,
            outcome: outcome(&state.status)?,
        });
    }
    Ok(OutcomeSummary { runs })
}

pub(super) async fn run(
    layout: &bureau::home::Layout,
    settings: &Settings,
    config: &ValidatedConfig,
    migration: Option<&migrate::Prepared>,
) -> anyhow::Result<OutcomeSummary> {
    let durable = durable_paths(layout, migration);
    let before = run_states(&durable.runs)?;
    let staging = Temporary::new(layout.config_cache(), "init-reconcile")?;
    let settings_path = staging.path().join("settings.yaml");
    bureau::setup::save_settings(&settings_path, settings)?;
    reconcile::run(arguments(layout, settings, config, settings_path, &durable)).await?;
    summaries(&durable.runs, &before)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{durable_paths, migrate};

    #[test]
    fn migration_routes_first_pass_to_staged_durable_paths() {
        let root = std::env::temp_dir().join(format!("bureau-pass-{}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("runs")).expect("source runs");
        let home = bureau::home::Home::new(root.join("home"));
        let settings = migration_settings(source);
        let prepared = migrate::prepare(home.layout(), &settings)
            .expect("migration")
            .expect("prepared migration");
        let paths = durable_paths(home.layout(), Some(&prepared));
        let staged = paths.runs != home.layout().runs() && paths.state != home.layout().state_db();
        drop(prepared);
        std::fs::remove_dir_all(root).expect("cleanup");
        assert!(staged);
    }

    #[test]
    fn effects_running_migration_resumes_the_same_stage() {
        let root = std::env::temp_dir().join(format!("bureau-resume-{}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("runs")).expect("source runs");
        let home = bureau::home::Home::new(root.join("home"));
        let mut settings = migration_settings(source);
        let first = migrate::prepare(home.layout(), &settings)
            .expect("migration")
            .expect("prepared");
        let first_runs = first.runs_path();
        migrate::Prepared::before_effects(home.layout()).expect("effects phase");
        drop(first);
        migrate::recover_pending(home.layout(), Some(&mut settings)).expect("pending");
        let resumed = migrate::prepare(home.layout(), &settings)
            .expect("resume")
            .expect("prepared");
        let same = resumed.runs_path() == first_runs;
        drop(resumed);
        std::fs::remove_dir_all(root).expect("cleanup");
        assert!(same);
    }

    #[test]
    fn effects_running_migration_rejects_changed_settings() {
        let root = std::env::temp_dir().join(format!("bureau-changed-{}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("runs")).expect("source runs");
        let home = bureau::home::Home::new(root.join("home"));
        let mut settings = migration_settings(source);
        let prepared = migrate::prepare(home.layout(), &settings)
            .expect("migration")
            .expect("prepared");
        migrate::Prepared::before_effects(home.layout()).expect("effects phase");
        drop(prepared);
        settings.config = bureau::setup::ConfigSource::SeparateRepository {
            remote: "changed".to_owned(),
            reference: "main".to_owned(),
        };
        let rejected = migrate::recover_pending(home.layout(), Some(&mut settings)).is_err();
        std::fs::remove_file(home.layout().root().join("migration.json")).expect("marker");
        std::fs::remove_dir_all(root).expect("cleanup");
        assert!(rejected);
    }

    fn migration_settings(source: std::path::PathBuf) -> bureau::setup::Settings {
        bureau::setup::Settings {
            config: bureau::setup::ConfigSource::SeparateRepository {
                remote: "config".to_owned(),
                reference: "main".to_owned(),
            },
            credentials: BTreeMap::new(),
            plugin: bureau::setup::PluginSettings::default(),
            migration: bureau::setup::MigrationSettings {
                source: Some(source),
            },
        }
    }
}
