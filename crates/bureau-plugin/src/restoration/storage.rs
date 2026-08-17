use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Error;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginalState {
    pub bytes: Vec<u8>,
    pub mode: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileState {
    pub path: PathBuf,
    pub original: Option<OriginalState>,
    pub injected: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub plugin: String,
    pub version: String,
    pub worktree: PathBuf,
    pub source: Option<PathBuf>,
    pub files: Vec<FileState>,
    pub directories: Vec<PathBuf>,
    pub temporary_roots: Vec<PathBuf>,
}

fn valid_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn validate_id(value: &str) -> Result<(), Error> {
    if valid_id(value) {
        Ok(())
    } else {
        Err(Error::invalid(
            Path::new(value),
            "invalid activation identity",
        ))
    }
}

pub fn is_finalized(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("json")
        && path
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(valid_id)
}

pub fn read(path: &Path) -> Result<Manifest, Error> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| Error::io("inspect restoration", path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.nlink() > 1 {
        return Err(Error::invalid(
            path,
            "restoration record is not a safe file",
        ));
    }
    let bytes = fs::read(path).map_err(|error| Error::io("read restoration", path, error))?;
    serde_json::from_slice(&bytes).map_err(|error| Error::invalid(path, error))
}

fn write_temporary(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| Error::io("create restoration", path, error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| Error::io("write restoration", path, error))
}

pub fn write(path: &Path, manifest: &Manifest) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid(path, "restoration path has no parent"))?;
    let temporary = path.with_extension("json.tmp");
    let _removed = fs::remove_file(&temporary);
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| Error::invalid(path, error))?;
    write_temporary(&temporary, &bytes)?;
    fs::rename(&temporary, path).map_err(|error| Error::io("replace restoration", path, error))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| Error::io("sync restoration root", parent, error))
}
