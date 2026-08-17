//! Production diagnostics backed only by local files and process metadata.

mod config;
mod recovery;
mod system;

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::home::{Environment, Layout};
use crate::setup::{Settings, load_settings};

use super::{Area, Effects, Observation};

/// The executable search path from the process environment, read
/// through the lifecycle crate's environment boundary.
fn system_search_path() -> Vec<PathBuf> {
    crate::home::ProcessEnvironment
        .value("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

/// Every environment-variable name, without values. The process
/// environment boundary: `vars_os` is bound once as a function pointer
/// so this stays the single read site.
fn environment_names() -> BTreeSet<OsString> {
    let vars = std::env::vars_os;
    vars().map(|(name, _value)| name).collect()
}

fn bundled_plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/bureau")
}

pub(super) fn replay_run_read_only(
    directory: &std::path::Path,
) -> Result<crate::runlog::RunState, String> {
    recovery::replay_run_read_only(directory)
}

/// Read-only production doctor effects for one resolved local layout.
pub struct LocalEffects {
    pub(super) layout: Layout,
    pub(super) settings: Result<Settings, String>,
    pub(super) search_path: Vec<PathBuf>,
    pub(super) environment_names: BTreeSet<OsString>,
    pub(super) plugin_root: PathBuf,
}

impl LocalEffects {
    /// Captures local, non-secret process metadata for one doctor run.
    #[must_use]
    pub fn new(layout: &Layout) -> Self {
        Self {
            layout: layout.clone(),
            settings: load_settings(layout.settings()).map_err(|error| error.to_string()),
            search_path: system_search_path(),
            environment_names: environment_names(),
            plugin_root: bundled_plugin_root(),
        }
    }

    /// Overrides executable search paths for a contained environment.
    #[must_use]
    pub fn search_path(mut self, value: &OsStr) -> Self {
        self.search_path = std::env::split_paths(value).collect();
        self
    }

    /// Overrides the captured environment-variable names without values.
    #[must_use]
    pub fn environment_names(mut self, names: impl IntoIterator<Item = OsString>) -> Self {
        self.environment_names = names.into_iter().collect();
        self
    }

    /// Overrides the bundled plugin root.
    #[must_use]
    pub fn bundled_plugin(mut self, root: impl Into<PathBuf>) -> Self {
        self.plugin_root = root.into();
        self
    }

    pub(super) fn settings(&self) -> Result<&Settings, String> {
        self.settings.as_ref().map_err(Clone::clone)
    }
}

impl Effects for LocalEffects {
    fn inspect(&self, area: Area) -> Result<Observation, String> {
        match area {
            Area::LocalState => self.inspect_local_state(),
            Area::ConfigSource => self.inspect_config_source(),
            Area::Repositories => self.inspect_repositories(),
            Area::CredentialReferences => self.inspect_credentials(),
            Area::Adapters => self.inspect_adapters(),
            Area::PluginsAndMcp => Ok(self.inspect_plugin()),
            Area::RecoveryState => self.inspect_recovery(),
        }
    }
}
