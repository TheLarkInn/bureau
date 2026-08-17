//! Local marketplace catalog lookup and temporary entry injection.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::settings::Marketplace;
use super::{Error, json, paths};

fn named(value: &Value, name: &str) -> bool {
    value.get("name").and_then(Value::as_str) == Some(name)
}

fn new_entry(name: &str, version: &str, source: &str) -> Value {
    let mut entry = Map::new();
    entry.insert("name".to_owned(), Value::String(name.to_owned()));
    entry.insert("version".to_owned(), Value::String(version.to_owned()));
    entry.insert("source".to_owned(), Value::String(source.to_owned()));
    Value::Object(entry)
}

fn update_entry(value: &mut Value, path: &Path, version: &str, source: &str) -> Result<(), Error> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::invalid(path, "plugin catalog entry must be an object"))?;
    object.insert("version".to_owned(), Value::String(version.to_owned()));
    object.insert("source".to_owned(), Value::String(source.to_owned()));
    Ok(())
}

fn plugins_mut<'a>(value: &'a mut Value, path: &Path) -> Result<&'a mut Vec<Value>, Error> {
    value
        .get_mut("plugins")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::invalid(path, "marketplace `plugins` must be an array"))
}

fn plugin_entry<'a>(
    value: &'a Value,
    plugin: &str,
    path: &Path,
) -> Result<Option<&'a Value>, Error> {
    let entries = value
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid(path, "marketplace `plugins` must be an array"))?;
    Ok(entries.iter().find(|entry| named(entry, plugin)))
}

pub fn new_marketplace() -> Value {
    serde_json::json!({
        "name": "repo-plugins",
        "owner": { "name": "bureau" },
        "plugins": []
    })
}

pub fn plugin_path(marketplace: &Marketplace, plugin: &str) -> Result<Option<PathBuf>, Error> {
    let value = json::read(&marketplace.catalog)?;
    let Some(entry) = plugin_entry(&value, plugin, &marketplace.catalog)? else {
        return Ok(None);
    };
    let source = entry.get("source").and_then(Value::as_str).ok_or_else(|| {
        Error::invalid(
            &marketplace.catalog,
            format!("plugin `{plugin}` must use a local string source"),
        )
    })?;
    paths::contained_existing(&marketplace.root, Path::new(source))
}

pub fn inject(
    value: &mut Value,
    path: &Path,
    plugin: &str,
    version: &str,
    source: &str,
) -> Result<(), Error> {
    let entries = plugins_mut(value, path)?;
    if let Some(entry) = entries.iter_mut().find(|entry| named(entry, plugin)) {
        update_entry(entry, path, version, source)?;
    } else {
        entries.push(new_entry(plugin, version, source));
    }
    Ok(())
}
