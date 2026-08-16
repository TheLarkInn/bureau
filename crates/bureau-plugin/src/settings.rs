//! Repository Copilot settings and local marketplace discovery.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{Error, json, paths};

pub const SETTINGS_PATH: &str = ".github/copilot/settings.json";

#[derive(Debug, Clone)]
pub struct Settings {
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct Marketplace {
    pub name: String,
    pub root: PathBuf,
    pub catalog: PathBuf,
}

impl Settings {
    pub fn read(worktree: &Path) -> Result<Self, Error> {
        let path = worktree.join(SETTINGS_PATH);
        let value = json::read_optional(&path)?.unwrap_or_else(empty_object);
        if value.is_object() {
            return Ok(Self { value });
        }
        Err(Error::invalid(&path, "settings must be a JSON object"))
    }

    pub fn local_marketplaces(&self, worktree: &Path) -> Result<Vec<Marketplace>, Error> {
        let mut found = Vec::new();
        for (name, relative) in self.local_specs()? {
            if let Some(marketplace) = existing_marketplace(worktree, name, &relative)? {
                found.push(marketplace);
            }
        }
        Ok(found)
    }

    pub fn plugin_enabled(&self, plugin: &str, marketplace: &str) -> bool {
        let key = format!("{plugin}@{marketplace}");
        self.value
            .get("enabledPlugins")
            .and_then(Value::as_object)
            .and_then(|plugins| plugins.get(&key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn local_specs(&self) -> Result<BTreeMap<String, PathBuf>, Error> {
        let Some(entries) = self.value.get("extraKnownMarketplaces") else {
            return Ok(BTreeMap::new());
        };
        let entries = entries.as_object().ok_or_else(|| {
            Error::invalid(Path::new(SETTINGS_PATH), "marketplaces must be an object")
        })?;
        entries.iter().filter_map(local_spec).collect()
    }
}

pub fn register_local(value: &mut Value, name: &str, path: &str) -> Result<(), Error> {
    let marketplaces = object_field(value, "extraKnownMarketplaces")?;
    marketplaces.insert(name.to_owned(), marketplace_value(path));
    Ok(())
}

pub fn enable(value: &mut Value, plugin: &str, marketplace: &str) -> Result<(), Error> {
    let plugins = object_field(value, "enabledPlugins")?;
    plugins.insert(format!("{plugin}@{marketplace}"), Value::Bool(true));
    Ok(())
}

fn local_spec(entry: (&String, &Value)) -> Option<Result<(String, PathBuf), Error>> {
    let (name, value) = entry;
    let source = value.get("source")?;
    if source.get("source").and_then(Value::as_str) != Some("directory") {
        return None;
    }
    Some(local_path(name, source))
}

fn local_path(name: &str, source: &Value) -> Result<(String, PathBuf), Error> {
    let Some(path) = source.get("path").and_then(Value::as_str) else {
        return Err(Error::invalid(
            Path::new(SETTINGS_PATH),
            format!("local marketplace `{name}` has no directory path"),
        ));
    };
    Ok((name.to_owned(), PathBuf::from(path)))
}

fn existing_marketplace(
    worktree: &Path,
    name: String,
    relative: &Path,
) -> Result<Option<Marketplace>, Error> {
    let Some(root) = paths::contained_existing(worktree, relative)? else {
        return Ok(None);
    };
    let Some(catalog) = catalog_path(&root) else {
        return Ok(None);
    };
    Ok(Some(Marketplace {
        name,
        root,
        catalog,
    }))
}

fn catalog_path(root: &Path) -> Option<PathBuf> {
    [
        root.join(".github/plugin/marketplace.json"),
        root.join(".claude-plugin/marketplace.json"),
        root.join("marketplace.json"),
    ]
    .into_iter()
    .find(|path| fs::symlink_metadata(path).is_ok())
}

fn object_field<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>, Error> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::invalid(Path::new(SETTINGS_PATH), "settings must be an object"))?;
    let field = object
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    field.as_object_mut().ok_or_else(|| {
        Error::invalid(
            Path::new(SETTINGS_PATH),
            format!("`{key}` must be an object"),
        )
    })
}

fn marketplace_value(path: &str) -> Value {
    serde_json::json!({
        "source": {
            "source": "directory",
            "path": path
        }
    })
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}
