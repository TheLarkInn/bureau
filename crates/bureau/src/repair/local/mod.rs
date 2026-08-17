//! Production reversible effects backed by one resolved local layout.

mod files;
mod state;

use crate::home::{Directory, Layout};

use super::{DisposableCache, Effects, Ownership};

/// Production repair effects with no policy or credential mutation surface.
pub struct LocalEffects {
    layout: Layout,
}

impl LocalEffects {
    /// Uses the fixed paths from one resolved local home.
    #[must_use]
    pub fn new(layout: &Layout) -> Self {
        Self {
            layout: layout.clone(),
        }
    }
}

impl Effects for LocalEffects {
    fn create_directory(&mut self, directory: Directory) -> Result<(), String> {
        files::create_directory(&self.layout, directory)
    }

    fn fix_directory_permissions(&mut self, directory: Directory) -> Result<(), String> {
        files::fix_permissions(&self.layout, directory)
    }

    fn clear_cache(&mut self, cache: DisposableCache) -> Result<(), String> {
        if state::has_any_live_run(&self.layout)? {
            return Err("live run evidence prevents cache clearing".to_owned());
        }
        files::clear_cache(&self.layout, cache)
    }

    fn restore_plugin_activation(
        &mut self,
        run_id: &str,
        activation_id: &str,
        plugin: &str,
        version: &str,
    ) -> Result<(), String> {
        if state::has_live_run(&self.layout, run_id)? {
            return Err(format!("run `{run_id}` still has live evidence"));
        }
        let run = files::safe_run_directory(&self.layout, run_id)?;
        bureau_plugin::restore_stale(&run, activation_id, plugin, version)
            .map_err(|error| error.to_string())
    }

    fn reap_expired_ownership(&mut self, ownership: &Ownership) -> Result<(), String> {
        state::reap_expired(&self.layout, ownership)
    }

    fn prune_orphan_worktree(&mut self, run_id: &str) -> Result<(), String> {
        if state::has_live_run(&self.layout, run_id)? {
            return Err(format!("run `{run_id}` still has live evidence"));
        }
        files::prune_orphan_worktree(&self.layout, run_id)
    }

    fn rebuild_derived_state(&mut self, run_id: &str) -> Result<(), String> {
        state::rebuild_derived(&self.layout, run_id)
    }
}
