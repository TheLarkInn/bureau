use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use ring::digest::{SHA256, digest};

use crate::Error;

pub const DIRECTORY: &str = "activations";

pub fn canonical_directory(path: &Path) -> Result<PathBuf, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|error| Error::io("inspect", path, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::invalid(path, "expected a safe directory"));
    }
    fs::canonicalize(path).map_err(|error| Error::io("resolve", path, error))
}

fn canonical_source(path: &Path) -> Result<PathBuf, Error> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && path.is_absolute() => {
            Ok(path.to_path_buf())
        }
        Err(error) => Err(Error::io("resolve", path, error)),
    }
}

fn validate_relative(path: &Path) -> Result<(), Error> {
    let valid = path.components().next().is_some()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if valid {
        Ok(())
    } else {
        Err(Error::invalid(path, "restoration path must be relative"))
    }
}

pub fn activation_root(run_dir: &Path) -> Result<PathBuf, Error> {
    let root = run_dir.join(DIRECTORY);
    fs::create_dir_all(&root).map_err(|error| Error::io("create restorations", &root, error))?;
    Ok(root)
}

pub fn record_path(run_dir: &Path, activation_id: &str) -> PathBuf {
    run_dir
        .join(DIRECTORY)
        .join(format!("{activation_id}.json"))
}

pub fn contained_worktree(run_dir: &Path, worktree: &Path) -> Result<(PathBuf, PathBuf), Error> {
    let run_dir = canonical_directory(run_dir)?;
    let worktree = canonical_directory(worktree)?;
    if worktree.starts_with(&run_dir) && worktree != run_dir {
        Ok((run_dir, worktree))
    } else {
        Err(Error::invalid(
            worktree.as_path(),
            "activation worktree must be inside the run directory",
        ))
    }
}

pub fn canonical_optional(path: Option<&Path>) -> Result<Option<PathBuf>, Error> {
    path.map(canonical_source).transpose()
}

pub fn activation_id(relative: &Path) -> String {
    let bytes = relative.as_os_str().as_encoded_bytes();
    let mut output = String::with_capacity(64);
    for byte in digest(&SHA256, bytes).as_ref() {
        write!(output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

pub fn absolute(base: &Path, relative: &Path) -> Result<PathBuf, Error> {
    validate_relative(relative)?;
    Ok(base.join(relative))
}

pub fn relative(base: &Path, path: &Path) -> Result<PathBuf, Error> {
    let relative = path
        .strip_prefix(base)
        .map_err(|_| Error::invalid(path, "activation path escapes its root"))?;
    validate_relative(relative)?;
    Ok(relative.to_path_buf())
}

pub fn absolute_paths(worktree: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
    paths.iter().map(|path| absolute(worktree, path)).collect()
}

pub fn reject_existing(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io("inspect restoration", path, error)),
        Ok(_) => Err(Error::invalid(
            path,
            "stale activation exists; run `bureau repair`",
        )),
    }
}
