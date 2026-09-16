//! Snapshot-only resolution. These entry points never activate repository files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::reference::AgentReference;
use super::settings::{Marketplace, SETTINGS_PATH, Settings};
use super::snapshot::Snapshot;
use super::{Error, PluginSource, Resolved, Resolver, catalog, json, paths, snapshot};

fn plugin_name(name: &str) -> Result<(), Error> {
    AgentReference::parse(&format!("/{name}:snapshot")).map(|_| ())
}

/// A verified run snapshot, without a worktree activation guard.
#[derive(Debug, Clone)]
pub struct PluginSnapshot {
    /// Exact source identity recorded by the existing plugin resolver.
    pub source: PluginSource,
    /// Existing whole-package copy under the run directory.
    pub directory: PathBuf,
}

impl From<Snapshot> for PluginSnapshot {
    fn from(snapshot: Snapshot) -> Self {
        Self {
            source: snapshot.source,
            directory: snapshot.root,
        }
    }
}

fn object<'a>(
    value: &'a Value,
    field: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, Error> {
    value
        .get(field)
        .map(|entry| {
            entry.as_object().ok_or_else(|| {
                Error::invalid(
                    Path::new(SETTINGS_PATH),
                    format!("`{field}` must be an object"),
                )
            })
        })
        .transpose()
}

fn checked_settings(worktree: &Path) -> Result<Settings, Error> {
    paths::contained_existing(worktree, Path::new(SETTINGS_PATH))?;
    let settings = Settings::read(worktree)?;
    for field in [
        "extraKnownMarketplace",
        "hooks",
        "mcpServers",
        "mcp-servers",
    ] {
        if settings.value.get(field).is_some() {
            return Err(Error::invalid(
                Path::new(SETTINGS_PATH),
                format!(
                    "unsupported factory repository `{field}`; use extraKnownMarketplaces and audited plugin material"
                ),
            ));
        }
    }
    object(&settings.value, "extraKnownMarketplaces")?;
    Ok(settings)
}

fn enabled_entry(key: &str, value: &Value) -> Result<Option<(String, String)>, Error> {
    let enabled = value.as_bool().ok_or_else(|| {
        Error::invalid(
            Path::new(SETTINGS_PATH),
            format!("enabledPlugins `{key}` must be boolean"),
        )
    })?;
    let (plugin, marketplace) = key.split_once('@').ok_or_else(|| {
        Error::invalid(
            Path::new(SETTINGS_PATH),
            format!("enabled plugin `{key}` needs plugin@marketplace"),
        )
    })?;
    plugin_name(plugin)?;
    plugin_name(marketplace)?;
    Ok(enabled.then(|| (plugin.to_owned(), marketplace.to_owned())))
}

fn insert_enabled(
    plugins: &mut BTreeMap<String, String>,
    key: &str,
    value: &Value,
) -> Result<(), Error> {
    let Some((plugin, marketplace)) = enabled_entry(key, value)? else {
        return Ok(());
    };
    if plugins.insert(plugin.clone(), marketplace).is_some() {
        return Err(Error::invalid(
            Path::new(SETTINGS_PATH),
            format!("plugin `{plugin}` is enabled more than once"),
        ));
    }
    Ok(())
}

fn enabled(settings: &Settings) -> Result<BTreeMap<String, String>, Error> {
    let mut plugins = BTreeMap::new();
    for (key, value) in object(&settings.value, "enabledPlugins")?
        .into_iter()
        .flatten()
    {
        insert_enabled(&mut plugins, key, value)?;
    }
    Ok(plugins)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectorySource {
    source: String,
    path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketplaceSource {
    source: DirectorySource,
}

fn require_directory_source(settings: &Settings, name: &str) -> Result<(), Error> {
    let value = settings
        .value
        .get("extraKnownMarketplaces")
        .and_then(|entries| entries.get(name))
        .cloned()
        .unwrap_or_default();
    let entry: MarketplaceSource = serde_json::from_value(value).map_err(|error| {
        Error::invalid(
            Path::new(SETTINGS_PATH),
            format!("marketplace `{name}` needs an explicit local directory source: {error}"),
        )
    })?;
    if entry.source.source == "directory" && !entry.source.path.as_os_str().is_empty() {
        return Ok(());
    }
    Err(Error::invalid(
        Path::new(SETTINGS_PATH),
        format!(
            "enabled marketplace `{name}` requires an explicit local directory; remote or missing material is not installed during a factory run"
        ),
    ))
}

fn marketplace(settings: &Settings, worktree: &Path, name: &str) -> Result<Marketplace, Error> {
    require_directory_source(settings, name)?;
    settings.local_marketplaces(worktree)?.into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| Error::invalid(
            Path::new(SETTINGS_PATH),
            format!("marketplace `{name}` needs an existing contained directory and local catalog; provision it before retrying"),
        ))
}

fn entry_count(value: &Value, plugin: &str, catalog: &Path) -> Result<usize, Error> {
    let entries = value
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid(catalog, "marketplace `plugins` must be an array"))?;
    Ok(entries
        .iter()
        .filter(|entry| entry.get("name").and_then(Value::as_str) == Some(plugin))
        .count())
}

fn unique_entry(marketplace: &Marketplace, plugin: &str) -> Result<(), Error> {
    let relative = marketplace
        .catalog
        .strip_prefix(&marketplace.root)
        .map_err(|error| Error::invalid(&marketplace.catalog, error))?;
    paths::contained_existing(&marketplace.root, relative)?;
    let value = json::read(&marketplace.catalog)?;
    if entry_count(&value, plugin, &marketplace.catalog)? == 1 {
        return Ok(());
    }
    Err(Error::invalid(
        &marketplace.catalog,
        format!("plugin `{plugin}` needs exactly one explicit local catalog entry"),
    ))
}

fn local_source(
    settings: &Settings,
    worktree: &Path,
    plugin: &str,
    name: &str,
) -> Result<Resolved, Error> {
    let marketplace = marketplace(settings, worktree, name)?;
    unique_entry(&marketplace, plugin)?;
    let path = catalog::plugin_path(&marketplace, plugin)?.ok_or_else(|| {
        Error::invalid(
            &marketplace.catalog,
            format!("plugin `{plugin}` has no existing contained local material; provision it before retrying"),
        )
    })?;
    Ok(Resolved {
        path,
        description: format!("target repository marketplace `{name}`"),
    })
}

fn local_snapshot(
    run_dir: &Path,
    plugin: &str,
    resolved: &Resolved,
) -> Result<PluginSnapshot, Error> {
    if let Some(existing) = snapshot::load(run_dir, plugin)? {
        if existing.source.source != resolved.description {
            return Err(Error::invalid(
                &existing.root,
                "enabled repository plugin conflicts with the existing run snapshot",
            ));
        }
        return Ok(existing.into());
    }
    snapshot::create(run_dir, plugin, resolved).map(Into::into)
}

impl Resolver {
    /// Reopens an expected source without consulting repository or user settings.
    ///
    /// # Errors
    /// Rejects absent, changed, unsafe, or differently identified snapshots.
    pub fn require_snapshot(&self, expected: &PluginSource) -> Result<PluginSnapshot, Error> {
        plugin_name(&expected.name)?;
        let existing = snapshot::load(&self.run_dir, &expected.name)?.ok_or_else(|| {
            Error::invalid(&self.run_dir, format!("pinned plugin `{}` is missing; restore its original run material, not current sources", expected.name))
        })?;
        if &existing.source != expected {
            return Err(Error::invalid(
                &existing.root,
                "plugin source differs from the immutable run plan",
            ));
        }
        Ok(existing.into())
    }

    /// Uses normal trusted role-source precedence, without temporary activation.
    ///
    /// # Errors
    /// Rejects invalid references, unavailable material, or corrupt existing pins.
    pub fn snapshot_only(&self, reference: &str, worktree: &Path) -> Result<PluginSnapshot, Error> {
        let reference = AgentReference::parse(reference)?;
        paths::ensure_outside(&self.run_dir, worktree)?;
        if let Some(existing) = snapshot::load(&self.run_dir, &reference.plugin)? {
            return Ok(existing.into());
        }
        paths::contained_existing(worktree, Path::new(SETTINGS_PATH))?;
        let settings = Settings::read(worktree)?;
        self.snapshot(&reference, worktree, &settings)
            .map(Into::into)
    }

    /// Snapshots only explicitly enabled, materialized local repository plugins.
    ///
    /// The selected role's already-pinned plugin is retained independently of
    /// changed repository declarations. Other plugins never fall back to global
    /// or development sources. Unrelated repository settings are not activated.
    ///
    /// # Errors
    /// Rejects malformed declarations, remote sources, missing material, or conflicts.
    pub fn repository_snapshots(
        &self,
        worktree: &Path,
        selected_plugin: Option<&str>,
    ) -> Result<Vec<PluginSnapshot>, Error> {
        paths::ensure_outside(&self.run_dir, worktree)?;
        let settings = checked_settings(worktree)?;
        let mut snapshots = Vec::new();
        for (plugin, name) in enabled(&settings)? {
            if Some(plugin.as_str()) == selected_plugin {
                continue;
            }
            let resolved = local_source(&settings, worktree, &plugin, &name)?;
            snapshots.push(local_snapshot(&self.run_dir, &plugin, &resolved)?);
        }
        Ok(snapshots)
    }
}
