use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Component, Path};

use crate::home::{Directory, Environment, Layout};
use crate::repair::DisposableCache;
use crate::runlog;

const DIRECTORY_MODE: u32 = 0o700;

/// The executable search path inherited by the child process, read
/// through the lifecycle crate's environment boundary.
fn search_path() -> std::ffi::OsString {
    crate::home::ProcessEnvironment
        .value("PATH")
        .unwrap_or_default()
}

fn validate_directory(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(format!("{} is not a safe directory", path.display()))
    }
}

fn set_mode(path: &Path) -> Result<(), String> {
    let permissions = fs::Permissions::from_mode(DIRECTORY_MODE);
    fs::set_permissions(path, permissions).map_err(|error| format!("{}: {error}", path.display()))
}

fn ensure_safe_root(layout: &Layout) -> Result<(), String> {
    let root = layout.root();
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(format!("{} is not a safe directory", root.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(root).map_err(|error| error.to_string())?;
            validate_directory(root)
        }
        Err(error) => Err(format!("{}: {error}", root.display())),
    }
}

fn remove_entry(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))
    } else {
        fs::remove_file(path).map_err(|error| format!("{}: {error}", path.display()))
    }
}

pub(super) fn create_directory(layout: &Layout, directory: Directory) -> Result<(), String> {
    ensure_safe_root(layout)?;
    let path = layout.directory(directory);
    fs::create_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))?;
    validate_directory(path)?;
    set_mode(path)
}

pub(super) fn fix_permissions(layout: &Layout, directory: Directory) -> Result<(), String> {
    ensure_safe_root(layout)?;
    let path = layout.directory(directory);
    validate_directory(path)?;
    set_mode(path)
}

pub(super) fn clear_cache(layout: &Layout, cache: DisposableCache) -> Result<(), String> {
    let path = match cache {
        DisposableCache::Checkout => layout.checkout_cache(),
        DisposableCache::Config => layout.config_cache(),
    };
    validate_directory(path)?;
    let entries = fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?;
    for entry in entries {
        remove_entry(&entry.map_err(|error| error.to_string())?.path())?;
    }
    set_mode(path)
}

fn contained_mirror(cache: &Path, mirror: &Path) -> Result<std::path::PathBuf, String> {
    let cache = fs::canonicalize(cache).map_err(|error| error.to_string())?;
    let mirror = fs::canonicalize(mirror).map_err(|error| error.to_string())?;
    if mirror.starts_with(&cache) {
        Ok(mirror)
    } else {
        Err("worktree registration is outside the checkout cache".to_owned())
    }
}

fn registered_mirror(layout: &Layout, git_file: &Path) -> Result<std::path::PathBuf, String> {
    let content = fs::read_to_string(git_file).map_err(|error| error.to_string())?;
    let value = content
        .trim()
        .strip_prefix("gitdir: ")
        .ok_or_else(|| format!("{} has invalid worktree metadata", git_file.display()))?;
    let registration = Path::new(value);
    let mirror = registration
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "worktree registration has no mirror".to_owned())?;
    contained_mirror(layout.checkout_cache(), mirror)
}

fn run_git_remove(mirror: &Path, worktree: &Path) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .current_dir(mirror)
        .env_clear()
        .env("PATH", search_path())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn remove_registered_worktree(layout: &Layout, worktree: &Path) -> Result<bool, String> {
    let git_file = worktree.join(".git");
    let metadata = match fs::symlink_metadata(&git_file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("{}: {error}", git_file.display())),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{} is not a safe git file", git_file.display()));
    }
    let mirror = registered_mirror(layout, &git_file)?;
    run_git_remove(&mirror, worktree)?;
    Ok(true)
}

fn remove_worktree(layout: &Layout, worktree: &Path) -> Result<(), String> {
    if remove_registered_worktree(layout, worktree)? {
        return Ok(());
    }
    fs::remove_dir_all(worktree).map_err(|error| format!("{}: {error}", worktree.display()))
}

pub(super) fn safe_run_directory(
    layout: &Layout,
    run_id: &str,
) -> Result<std::path::PathBuf, String> {
    let mut components = Path::new(run_id).components();
    let valid =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !valid {
        return Err("run id must be one safe path component".to_owned());
    }
    let directory = layout.runs().join(run_id);
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(directory),
        Err(error) => return Err(format!("{}: {error}", directory.display())),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{} is not a safe run directory",
            directory.display()
        ));
    }
    let root = fs::canonicalize(layout.runs()).map_err(|error| error.to_string())?;
    let resolved = fs::canonicalize(&directory).map_err(|error| error.to_string())?;
    if resolved.starts_with(root) {
        Ok(directory)
    } else {
        Err("run directory escapes the local run root".to_owned())
    }
}

pub(super) fn prune_orphan_worktree(layout: &Layout, run_id: &str) -> Result<(), String> {
    let directory = safe_run_directory(layout, run_id)?;
    if directory.join(runlog::EVENTS_FILE).exists() {
        return Err(format!("run `{run_id}` has durable event history"));
    }
    let worktree = directory.join("wt");
    match fs::symlink_metadata(&worktree) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            remove_worktree(layout, &worktree)
        }
        Ok(_) => Err(format!("{} is not a safe worktree", worktree.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", worktree.display())),
    }
}
