//! Atomic non-secret settings persistence.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use crate::home::Layout;

use super::{Settings, SettingsEffects};

/// Settings persistence failure.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// Filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// YAML encoding or decoding failed.
    #[error(transparent)]
    Yaml(#[from] serde_yaml_ng::Error),
}

/// Production local settings effects backed by the resolved home layout.
#[derive(Debug, Clone, Copy)]
pub struct FileEffects<'a> {
    layout: &'a Layout,
}

impl<'a> FileEffects<'a> {
    /// Uses the settings path from `layout`.
    #[must_use]
    pub const fn new(layout: &'a Layout) -> Self {
        Self { layout }
    }
}

impl SettingsEffects for FileEffects<'_> {
    type Error = FileError;

    fn settings_exist(&mut self) -> Result<bool, Self::Error> {
        let exists = self
            .layout
            .settings()
            .try_exists()
            .map_err(FileError::from)?;
        Ok(exists)
    }

    fn write_settings_atomically(&mut self, settings: &Settings) -> Result<(), Self::Error> {
        save_settings(self.layout.settings(), settings)
    }
}

/// Loads non-secret settings.
///
/// # Errors
/// Propagates filesystem and strict YAML errors.
pub fn load_settings(path: &Path) -> Result<Settings, FileError> {
    let bytes = fs::read(path)?;
    Ok(serde_yaml_ng::from_slice(&bytes)?)
}

/// Replaces non-secret settings atomically.
///
/// # Errors
/// Propagates serialization, filesystem, and durability failures.
pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), FileError> {
    let parent = settings_parent(path);
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("yaml.tmp");
    let _removed = fs::remove_file(&temporary);
    write_temporary(&temporary, &serde_yaml_ng::to_string(settings)?)?;
    fs::rename(&temporary, path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn settings_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn write_temporary(path: &Path, value: &str) -> Result<(), std::io::Error> {
    write_temporary_with(path, value, |file, text| {
        file.write_all(text.as_bytes())?;
        file.sync_all()
    })
}

fn write_temporary_with(
    path: &Path,
    value: &str,
    persist: impl FnOnce(&mut fs::File, &str) -> Result<(), std::io::Error>,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = persist(&mut file, value);
    drop(file);
    if result.is_err() {
        let _removed = fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::{Path, PathBuf};

    use super::write_temporary_with;

    fn failure_path() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/settings-effects");
        std::fs::create_dir_all(&root).expect("create project-local fixture");
        root.join(format!("write-failure-{}.tmp", std::process::id()))
    }

    #[test]
    fn failed_write_removes_temporary_file() {
        let path = failure_path();
        let _removed = std::fs::remove_file(&path);
        let result = write_temporary_with(&path, "settings", |_, _| {
            Err(io::Error::other("injected write failure"))
        });
        assert_eq!((result.is_err(), path.exists()), (true, false));
    }
}
