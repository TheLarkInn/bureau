//! Containment checks for files read from a committed config snapshot.

use std::collections::BTreeMap;
use std::path::Path;

use super::{AdapterKind, Role, SourceError};

const NAMED_DIRS: [&str; 4] = ["roles", "assignments", "label_rules", "pipelines"];

fn unsafe_path(path: &Path) -> SourceError {
    SourceError::UnsafeConfig(path.display().to_string())
}

fn is_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("yaml" | "yml")
    )
}

fn validate_optional(snapshot: &Path, path: &Path, directory: bool) -> Result<bool, SourceError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let expected = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if metadata.file_type().is_symlink() || !expected {
        return Err(unsafe_path(path));
    }
    let canonical = std::fs::canonicalize(path)?;
    if !canonical.starts_with(snapshot) {
        return Err(unsafe_path(path));
    }
    Ok(true)
}

fn direct_path(config: &Path, value: &str) -> Result<std::path::PathBuf, SourceError> {
    let path = Path::new(value);
    let unsafe_path = path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        });
    if unsafe_path {
        return Err(SourceError::UnsafeConfig(value.to_owned()));
    }
    Ok(config.join(path))
}

fn require_dir(snapshot: &Path, path: &Path) -> Result<(), SourceError> {
    validate_optional(snapshot, path, true)?;
    Ok(())
}

fn validate_named_dir(snapshot: &Path, directory: &Path) -> Result<(), SourceError> {
    if !validate_optional(snapshot, directory, true)? {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if is_yaml(&path) {
            validate_optional(snapshot, &path, false)?;
        }
    }
    Ok(())
}

fn load_agent(
    snapshot: &Path,
    config: &Path,
    name: &str,
    value: &str,
) -> Result<(String, Vec<u8>), SourceError> {
    let path = direct_path(config, value)?;
    if !validate_optional(snapshot, &path, false)? {
        return Err(unsafe_path(&path));
    }
    Ok((name.to_owned(), std::fs::read(path)?))
}

pub(super) fn validate(snapshot: &Path, config: &Path) -> Result<(), SourceError> {
    let snapshot = std::fs::canonicalize(snapshot)?;
    require_dir(&snapshot, config)?;
    validate_optional(&snapshot, &config.join("repos.yaml"), false)?;
    for name in NAMED_DIRS {
        validate_named_dir(&snapshot, &config.join(name))?;
    }
    Ok(())
}

pub(super) fn load_agent_files(
    snapshot: &Path,
    config: &Path,
    roles: &BTreeMap<String, Role>,
) -> Result<BTreeMap<String, Vec<u8>>, SourceError> {
    let snapshot = std::fs::canonicalize(snapshot)?;
    roles
        .iter()
        .filter(|(_, role)| {
            role.adapter != AdapterKind::Fake && !bureau_plugin::is_plugin_reference(&role.agent)
        })
        .map(|(name, role)| load_agent(&snapshot, config, name, &role.agent))
        .collect()
}
