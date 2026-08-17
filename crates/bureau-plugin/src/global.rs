//! Enabled user-global plugin lookup under `COPILOT_HOME`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Error, Resolved, json, paths};

fn read_directories(path: &Path) -> Result<Vec<PathBuf>, Error> {
    match fs::read_dir(path) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .map(|value| value.path())
                    .map_err(|error| Error::io("read directory", path, error))
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(Error::io("read directory", path, error)),
    }
}

fn contained_absolute(home: &Path, path: &Path) -> Result<PathBuf, Error> {
    let canonical_home =
        fs::canonicalize(home).map_err(|error| Error::io("resolve", home, error))?;
    let canonical = fs::canonicalize(path).map_err(|error| Error::io("resolve", path, error))?;
    if canonical.starts_with(canonical_home) {
        return Ok(path.to_path_buf());
    }
    Err(Error::invalid(
        path,
        "user-global plugin must stay under COPILOT_HOME",
    ))
}

fn enabled_record(record: &Value, plugin: &str) -> bool {
    record.get("name").and_then(Value::as_str) == Some(plugin)
        && record.get("enabled").and_then(Value::as_bool) != Some(false)
}

fn install_path(record: &Value) -> Option<&str> {
    ["cache_path", "cachePath", "installPath"]
        .into_iter()
        .find_map(|key| record.get(key).and_then(Value::as_str))
}

fn resolve_record(
    home: &Path,
    plugin: &str,
    marketplace: &str,
    path: &Path,
) -> Result<Resolved, Error> {
    let path = if path.is_absolute() {
        contained_absolute(home, path)?
    } else {
        paths::contained_path(home, path)?
    };
    Ok(Resolved {
        path,
        description: format!("user-global plugin `{plugin}@{marketplace}`"),
    })
}

fn record_path(home: &Path, plugin: &str, record: &Value) -> Result<Resolved, Error> {
    let marketplace = record
        .get("marketplace")
        .and_then(Value::as_str)
        .unwrap_or("_direct");
    let path = install_path(record).map_or_else(
        || {
            home.join("installed-plugins")
                .join(marketplace)
                .join(plugin)
        },
        PathBuf::from,
    );
    resolve_record(home, plugin, marketplace, &path)
}

fn first_existing(home: &Path, candidates: Vec<Resolved>) -> Result<Option<Resolved>, Error> {
    for candidate in candidates {
        if fs::symlink_metadata(&candidate.path).is_ok() {
            contained_absolute(home, &candidate.path)?;
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn scanned_plugin(
    home: &Path,
    marketplace: &Path,
    plugin: &str,
) -> Result<Option<Resolved>, Error> {
    let Some(name) = marketplace.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    let path = marketplace.join(plugin);
    if fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let path = contained_absolute(home, &path)?;
    Ok(Some(Resolved {
        path,
        description: format!("user-global plugin `{plugin}@{name}`"),
    }))
}

fn scan(home: &Path, plugin: &str) -> Result<Option<Resolved>, Error> {
    let installed = home.join("installed-plugins");
    let mut marketplaces = read_directories(&installed)?;
    marketplaces.sort();
    for marketplace in marketplaces {
        if let Some(found) = scanned_plugin(home, &marketplace, plugin)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn find_record(home: &Path, plugin: &str, records: &[Value]) -> Result<Option<Resolved>, Error> {
    let mut candidates = records
        .iter()
        .filter(|record| enabled_record(record, plugin))
        .map(|record| record_path(home, plugin, record))
        .collect::<Result<Vec<_>, _>>()?;
    candidates.sort_by(|left, right| left.description.cmp(&right.description));
    first_existing(home, candidates)
}

pub fn find(home: &Path, plugin: &str) -> Result<Option<Resolved>, Error> {
    let config_path = home.join("config.json");
    let Some(config) = json::read_optional(&config_path)? else {
        return scan(home, plugin);
    };
    let Some(records) = config.get("installedPlugins").and_then(Value::as_array) else {
        return scan(home, plugin);
    };
    find_record(home, plugin, records)
}
