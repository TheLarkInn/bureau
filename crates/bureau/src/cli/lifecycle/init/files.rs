use std::path::{Component, Path, PathBuf};

use bureau::setup::ConfigDraft;

/// Nanoseconds since the Unix epoch. The process clock boundary:
/// bound once as a function pointer so this stays the single read site.
fn now() -> u128 {
    let now = std::time::SystemTime::now;
    now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn validate_relative(path: &Path) -> anyhow::Result<()> {
    let valid = path.components().next().is_some()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    anyhow::ensure!(valid, "config path must be relative: {}", path.display());
    Ok(())
}

fn create_safe_directory(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "config parent is unsafe: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn reject_unsafe_target(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "config target is unsafe: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn writable_path(root: &Path, relative: &Path) -> anyhow::Result<PathBuf> {
    validate_relative(relative)?;
    let mut parent = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for component in &components[..components.len() - 1] {
        parent.push(component.as_os_str());
        create_safe_directory(&parent)?;
    }
    let target = root.join(relative);
    reject_unsafe_target(&target)?;
    Ok(target)
}

pub(super) fn materialize(root: &Path, draft: &ConfigDraft) -> anyhow::Result<()> {
    std::fs::create_dir_all(root)?;
    for (relative, bytes) in &draft.files {
        let path = writable_path(root, relative)?;
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

pub(super) struct Temporary {
    path: PathBuf,
}

impl Temporary {
    pub(super) fn new(root: &Path, label: &str) -> anyhow::Result<Self> {
        std::fs::create_dir_all(root)?;
        let unique = format!("{label}-{}-{}", std::process::id(), now());
        let path = root.join(unique);
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use bureau::setup::ConfigDraft;

    use super::materialize;

    #[test]
    fn materialize_creates_a_missing_config_subdirectory() {
        let outer = std::env::temp_dir().join(format!("bureau-materialize-{}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&outer);
        let root = outer.join(".bureau");
        let draft = ConfigDraft {
            files: BTreeMap::from([
                (PathBuf::from("repos.yaml"), b"repos: {}".to_vec()),
                (PathBuf::from("roles/implementer.yaml"), b"name: i".to_vec()),
            ]),
        };
        let written = materialize(&root, &draft);
        let both = written.is_ok()
            && root.join("repos.yaml").is_file()
            && root.join("roles/implementer.yaml").is_file();
        std::fs::remove_dir_all(&outer).expect("cleanup");
        assert!(both);
    }
}
