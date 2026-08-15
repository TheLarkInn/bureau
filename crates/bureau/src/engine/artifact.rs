//! Safe publication of agent-produced artifacts.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::path::{Component, Path, PathBuf};

use crate::contract::Artifact;
use crate::process::{ScrubWriter, Secret};

use super::gitcmd;

pub(super) async fn materialize(
    artifacts: &mut [Artifact],
    worktree: &Path,
    destination: &Path,
    secrets: &[Secret],
) -> Result<(), String> {
    unique_names(artifacts)?;
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for artifact in artifacts {
        let source = confined_source(worktree, &artifact.path)?;
        artifact.path = copy(&source, worktree, destination, &artifact.name, secrets).await?;
    }
    Ok(())
}

fn unique_names(artifacts: &[Artifact]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for artifact in artifacts {
        if !safe_name(&artifact.name) {
            return Err(format!(
                "artifact name {:?} must be one path segment",
                artifact.name
            ));
        }
        if !names.insert(&artifact.name) {
            return Err(format!("artifact name {:?} is duplicated", artifact.name));
        }
    }
    Ok(())
}

fn safe_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn confined_source(worktree: &Path, path: &Path) -> Result<PathBuf, String> {
    let candidate = candidate(worktree, path)?;
    let root = std::fs::canonicalize(worktree).map_err(|error| error.to_string())?;
    let source = std::fs::canonicalize(candidate).map_err(|error| error.to_string())?;
    if !source.starts_with(&root) || !source.is_file() {
        return Err(format!(
            "artifact path {} must be a file inside the worktree",
            path.display()
        ));
    }
    Ok(source)
}

fn candidate(worktree: &Path, path: &Path) -> Result<PathBuf, String> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(format!(
            "artifact path {} must not contain `..`",
            path.display()
        ));
    }
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        worktree.join(path)
    };
    if std::fs::symlink_metadata(&candidate)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err(format!(
            "artifact path {} must not be a symlink",
            path.display()
        ));
    }
    Ok(candidate)
}

async fn digest(worktree: &Path, source: &Path) -> Result<String, String> {
    let source = source.to_string_lossy();
    gitcmd::git(&["hash-object", "--", &source], worktree, &[]).await
}

async fn copy(
    source: &Path,
    worktree: &Path,
    destination: &Path,
    name: &str,
    secrets: &[Secret],
) -> Result<PathBuf, String> {
    let temporary = destination.join(format!(".publishing-{name}"));
    write_scrubbed(source, &temporary, secrets)?;
    finish_copy(&temporary, worktree, destination, name).await
}

fn write_scrubbed(source: &Path, temporary: &Path, secrets: &[Secret]) -> Result<(), String> {
    let input = File::open(source).map_err(|error| error.to_string())?;
    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temporary)
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(input);
    let mut writer = ScrubWriter::new(BufWriter::new(output), secrets);
    std::io::copy(&mut reader, &mut writer).map_err(|error| error.to_string())?;
    let writer = writer.finish().map_err(|error| error.to_string())?;
    writer
        .into_inner()
        .map_err(|error| error.to_string())?
        .sync_all()
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn finish_copy(
    temporary: &Path,
    worktree: &Path,
    destination: &Path,
    name: &str,
) -> Result<PathBuf, String> {
    let digest = digest(worktree, temporary).await?;
    let path = destination.join(format!("{digest}-{name}"));
    match std::fs::rename(temporary, &path) {
        Ok(()) => Ok(path),
        Err(_) if path.exists() => {
            let _ = std::fs::remove_file(temporary);
            Ok(path)
        }
        Err(error) => Err(error.to_string()),
    }
}
