//! Source precedence for a plugin's first durable snapshot.

use std::path::{Path, PathBuf};

use super::settings::Settings;
use super::{Error, catalog, global};

#[derive(Debug)]
pub struct Resolved {
    pub path: PathBuf,
    pub description: String,
}

pub fn find(
    plugin: &str,
    worktree: &Path,
    settings: &Settings,
    copilot_home: Option<&Path>,
) -> Result<Resolved, Error> {
    if let Some(source) = target(plugin, worktree, settings)? {
        return Ok(source);
    }
    if let Some(home) = copilot_home {
        if let Some(source) = global::find(home, plugin)? {
            return Ok(source);
        }
    }
    development(plugin).ok_or_else(|| Error::MissingPlugin(plugin.to_owned()))
}

fn target(plugin: &str, worktree: &Path, settings: &Settings) -> Result<Option<Resolved>, Error> {
    for marketplace in settings.local_marketplaces(worktree)? {
        if !settings.plugin_enabled(plugin, &marketplace.name) {
            continue;
        }
        if let Some(path) = catalog::plugin_path(&marketplace, plugin)? {
            return Ok(Some(Resolved {
                path,
                description: format!("target repository marketplace `{}`", marketplace.name),
            }));
        }
    }
    Ok(None)
}

fn development(plugin: &str) -> Option<Resolved> {
    if plugin != "bureau" {
        return None;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join("plugins/bureau");
    root.is_dir().then_some(Resolved {
        path: root,
        description: "development source checkout".to_owned(),
    })
}
