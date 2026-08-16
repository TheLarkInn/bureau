//! Path containment and plain-file checks.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::Error;

pub fn contained_existing(base: &Path, relative: &Path) -> Result<Option<PathBuf>, Error> {
    let candidate = lexical_join(base, relative)?;
    if !present(&candidate)? {
        return Ok(None);
    }
    ensure_canonical_containment(base, &candidate)?;
    Ok(Some(candidate))
}

fn present(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::io("inspect", path, error)),
    }
}

pub fn contained_path(base: &Path, relative: &Path) -> Result<PathBuf, Error> {
    lexical_join(base, relative)
}

pub fn ensure_outside(run_dir: &Path, worktree: &Path) -> Result<(), Error> {
    let run = resolved_run_dir(run_dir)?;
    let worktree =
        fs::canonicalize(worktree).map_err(|error| Error::io("resolve", worktree, error))?;
    if run.starts_with(&worktree) {
        return Err(Error::invalid(
            run_dir,
            "run directory must be outside the worktree",
        ));
    }
    Ok(())
}

fn resolved_run_dir(path: &Path) -> Result<PathBuf, Error> {
    let absolute = absolute(path)?;
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(Error::invalid(path, "run directory may not be a symlink"))
        }
        Ok(_) => fs::canonicalize(&absolute).map_err(|error| Error::io("resolve", path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => resolve_missing(&absolute),
        Err(error) => Err(Error::io("inspect", path, error)),
    }
}

fn resolve_missing(path: &Path) -> Result<PathBuf, Error> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => return append_suffix(ancestor, &metadata, suffix),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| Error::invalid(path, "run path has no existing ancestor"))?;
                suffix.push(name.to_os_string());
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| Error::invalid(path, "run path has no existing ancestor"))?;
            }
            Err(error) => return Err(Error::io("inspect", ancestor, error)),
        }
    }
}

fn append_suffix(
    ancestor: &Path,
    metadata: &fs::Metadata,
    mut suffix: Vec<std::ffi::OsString>,
) -> Result<PathBuf, Error> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::invalid(
            ancestor,
            "run path ancestor is not a plain directory",
        ));
    }
    let canonical =
        fs::canonicalize(ancestor).map_err(|error| Error::io("resolve", ancestor, error))?;
    suffix.reverse();
    Ok(suffix.iter().fold(canonical, |base, name| base.join(name)))
}

pub fn ensure_plain_dir(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(Error::invalid(path, "expected a directory, not a symlink")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|source| Error::io("create directory", path, source))
        }
        Err(error) => Err(Error::io("inspect", path, error)),
    }
}

fn lexical_join(base: &Path, relative: &Path) -> Result<PathBuf, Error> {
    if relative.as_os_str().is_empty() {
        return Err(Error::invalid(relative, "path is empty"));
    }
    if relative.components().all(safe_component) {
        return Ok(base.join(relative));
    }
    Err(Error::invalid(
        relative,
        "path must stay within its configured directory",
    ))
}

const fn safe_component(component: Component<'_>) -> bool {
    matches!(component, Component::Normal(_) | Component::CurDir)
}

fn ensure_canonical_containment(base: &Path, candidate: &Path) -> Result<(), Error> {
    let canonical_base =
        fs::canonicalize(base).map_err(|error| Error::io("resolve", base, error))?;
    let canonical_candidate =
        fs::canonicalize(candidate).map_err(|error| Error::io("resolve", candidate, error))?;
    if canonical_candidate.starts_with(canonical_base) {
        return Ok(());
    }
    Err(Error::invalid(
        candidate,
        "path escapes its configured directory",
    ))
}

fn absolute(path: &Path) -> Result<PathBuf, Error> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let current = std::env::current_dir().map_err(|error| Error::io("resolve", path, error))?;
        current.join(path)
    };
    normalize(&joined)
}

fn normalize(path: &Path) -> Result<PathBuf, Error> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !result.pop() {
                    return Err(Error::invalid(path, "path escapes its root"));
                }
            }
            Component::CurDir => {}
            value => result.push(value.as_os_str()),
        }
    }
    Ok(result)
}
