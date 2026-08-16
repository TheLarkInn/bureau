//! Production setup effects, including optional Copilot plugin installation.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use bureau::setup::{PluginSettings, Settings};

pub(super) async fn run(from: &Path) -> anyhow::Result<i32> {
    let settings = bureau::setup::load_settings(from).context("loading settings")?;
    let home = bureau::home::Home::discover()?;
    let layout = home.layout().clone();
    let runtime = tokio::runtime::Handle::current();
    let settings_path = home.layout().settings().to_path_buf();
    tokio::task::spawn_blocking(move || apply(settings, &layout, runtime))
        .await
        .context("joining setup flow")??;
    println!("setup complete: {}", settings_path.display());
    Ok(0)
}

fn apply(
    mut settings: Settings,
    layout: &bureau::home::Layout,
    runtime: tokio::runtime::Handle,
) -> Result<(), Error> {
    let maintenance = bureau::maintenance::exclusive(layout.root())?;
    super::migrate::recover_pending(layout, Some(&mut settings))?;
    let mut effects = LocalEffects::new(layout, runtime, maintenance);
    let mut flow = bureau::setup::SetupFlow::new(settings);
    flow.run(&mut effects).map_err(|error| match error {
        bureau::setup::FlowError::Effect(error) => error,
        other => Error::Flow(other.to_string()),
    })
}

struct LocalEffects {
    layout: bureau::home::Layout,
    runtime: tokio::runtime::Handle,
    _maintenance: bureau::maintenance::Guard,
    migration: Option<super::migrate::Prepared>,
}

impl LocalEffects {
    fn new(
        layout: &bureau::home::Layout,
        runtime: tokio::runtime::Handle,
        maintenance: bureau::maintenance::Guard,
    ) -> Self {
        Self {
            layout: layout.clone(),
            runtime,
            _maintenance: maintenance,
            migration: None,
        }
    }
}

impl bureau::setup::SettingsEffects for LocalEffects {
    type Error = Error;

    fn settings_exist(&mut self) -> Result<bool, Self::Error> {
        Ok(self.layout.settings().try_exists()?)
    }

    fn write_settings_atomically(&mut self, settings: &Settings) -> Result<(), Self::Error> {
        super::migrate::save_settings(&mut self.migration, &self.layout, settings)
            .map_err(Error::from)
    }
}

impl bureau::setup::PluginEffects for LocalEffects {
    type Error = Error;

    fn install_user_plugin(&mut self, _: &PluginSettings) -> Result<(), Self::Error> {
        install_user_plugin(&self.runtime)
    }
}

impl bureau::setup::MigrationEffects for LocalEffects {
    type Error = Error;

    fn migrate_local_state(&mut self, settings: &Settings) -> Result<(), Self::Error> {
        self.migration = super::migrate::prepare(&self.layout, settings)?;
        Ok(())
    }
}

pub(super) fn install_user_plugin(runtime: &tokio::runtime::Handle) -> Result<(), Error> {
    for command in bureau::plugin::install_commands(&source_root())? {
        let result = runtime.block_on(bureau::process::spawn(spawn_request(command)));
        let success =
            result.outcome == bureau::process::SpawnOutcome::Exited && result.exit_code == Some(0);
        bureau::plugin::validate_install_result(success, &result.stderr)?;
    }
    Ok(())
}

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn spawn_request(command: bureau::plugin::InstallCommand) -> bureau::process::SpawnRequest {
    bureau::process::SpawnRequest {
        argv: command.argv,
        dir: command.directory,
        env: process_environment(),
        stdin: Vec::new(),
        timeout: Duration::from_secs(300),
        secrets: Vec::new(),
        log: None,
        cancel: None,
    }
}

fn process_environment() -> std::collections::BTreeMap<String, String> {
    ["PATH", "HOME", "COPILOT_HOME", "XDG_CONFIG_HOME"]
        .into_iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_owned(), value))
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    File(#[from] bureau::setup::FileError),
    #[error(transparent)]
    Plugin(#[from] bureau::plugin::Error),
    #[error(transparent)]
    Migration(#[from] anyhow::Error),
    #[error(transparent)]
    Maintenance(#[from] bureau::maintenance::Error),
    #[error("{0}")]
    Flow(String),
}
