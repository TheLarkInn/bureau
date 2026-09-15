//! Verify preserved SDK workspace identity without changing its resume directory.
//! Invalid external state must not silently fall back to the process cwd.

mod fields;
mod timestamp;

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use serde_yaml_ng::{Mapping, Value};

use crate::runlog::copilot_factory::Record;

fn string(mapping: &Mapping, field: &str) -> Result<String, String> {
    match mapping.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        _ => Err(format!(
            "runtime workspace `{field}` is missing or must be a string"
        )),
    }
}

struct Metadata {
    id: String,
    cwd: String,
}

fn parse(bytes: &[u8]) -> Result<Metadata, String> {
    let value: Value = serde_yaml_ng::from_slice(bytes).map_err(|error| error.to_string())?;
    let mapping = value
        .as_mapping()
        .ok_or("workspace root must be a YAML mapping")?;
    fields::verify(mapping)?;
    Ok(Metadata {
        id: string(mapping, "id")?,
        cwd: string(mapping, "cwd")?,
    })
}

fn read(path: &Path) -> Result<Metadata, String> {
    super::plain_file(path)
        .map_err(|error| format!("preserved workspace metadata is unavailable: {error}"))?;
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "reading preserved workspace metadata {}: {error}",
            path.display()
        )
    })?;
    parse(&bytes).map_err(|error| {
        format!(
            "preserved workspace metadata {} is malformed: {error}",
            path.display()
        )
    })
}

fn directory(cwd: &str) -> Result<(PathBuf, fs::Metadata), String> {
    let path = Path::new(cwd);
    if cwd.trim().is_empty() || !path.is_absolute() {
        return Err("preserved runtime workspace cwd must be a nonblank host-absolute path".into());
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("preserved runtime workspace cwd is unavailable: {error}"))?;
    if !metadata.is_dir() {
        return Err("preserved runtime workspace cwd is not a directory".into());
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("resolving preserved runtime workspace cwd: {error}"))?;
    Ok((canonical, metadata))
}

pub(super) fn verify(record: &Record) -> Result<(), String> {
    let path = record.intent.paths.storage.session.join("workspace.yaml");
    let metadata = read(&path)?;
    if metadata.id.trim().is_empty() || metadata.id != record.intent.session_id {
        return Err(
            "preserved runtime workspace session id differs from the durable intent".into(),
        );
    }
    let (actual, directory) = directory(&metadata.cwd)?;
    let expected = &record.intent.workspace;
    if !expected.directory.is_absolute()
        || actual != expected.directory
        || directory.dev() != expected.device
        || directory.ino() != expected.inode
    {
        return Err("preserved runtime workspace cwd differs from the approved worktree; initialization refused".into());
    }
    Ok(())
}
