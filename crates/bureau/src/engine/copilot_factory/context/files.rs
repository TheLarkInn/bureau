use std::fs;
use std::path::{Component, Path, PathBuf};

use bureau_plugin::TreeSnapshot;

pub(super) fn canonical(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("resolve {}: {error}", path.display()))
}

pub(super) fn relative(path: &Path) -> Result<(), String> {
    if !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Ok(());
    }
    Err(format!(
        "{} must be a nonempty contained relative path",
        path.display()
    ))
}

pub(super) fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("inspect {}: {error}", path.display())),
    }
}

pub(super) fn outside(directory: &Path, worktree: &Path) -> Result<(), String> {
    TreeSnapshot::require_outside(directory, worktree).map_err(|error| error.to_string())
}

pub(super) fn private_root(directory: &Path, worktree: &Path) -> Result<PathBuf, String> {
    outside(directory, worktree)?;
    fs::create_dir_all(directory).map_err(|error| {
        format!(
            "create context pin directory {}: {error}",
            directory.display()
        )
    })?;
    canonical(directory)
}

pub(super) fn read(tree: &TreeSnapshot, path: &Path) -> Result<Vec<u8>, String> {
    tree.read(path).map_err(|error| error.to_string())
}

fn entry_files(root: &Path, entry: &fs::DirEntry) -> Result<Vec<PathBuf>, String> {
    let path = entry.path();
    let kind = entry.file_type().map_err(|error| error.to_string())?;
    if kind.is_dir() {
        return walk(root, &path);
    }
    if !kind.is_file() {
        return Err(format!("{} is not a plain pinned file", path.display()));
    }
    path.strip_prefix(root)
        .map(|path| vec![path.to_path_buf()])
        .map_err(|error| error.to_string())
}

fn walk(root: &Path, directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("read pinned directory {}: {error}", directory.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        paths.extend(entry_files(
            root,
            &entry.map_err(|error| error.to_string())?,
        )?);
    }
    paths.sort();
    Ok(paths)
}

pub(super) fn all(tree: &TreeSnapshot) -> Result<Vec<PathBuf>, String> {
    tree.verify().map_err(|error| error.to_string())?;
    let paths = walk(tree.directory(), tree.directory())?;
    tree.verify().map_err(|error| error.to_string())?;
    Ok(paths)
}

pub(super) fn selected(tree: &TreeSnapshot, paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let files = all(tree)?;
    let mut selected = Vec::new();
    for name in paths {
        let resolved = tree
            .path(Path::new(name))
            .map_err(|error| error.to_string())?;
        let resolved = canonical(&resolved)?;
        let path = resolved
            .strip_prefix(tree.directory())
            .map_err(|error| error.to_string())?;
        selected.extend(files.iter().filter(|file| file.starts_with(path)).cloned());
    }
    selected.sort();
    selected.dedup();
    Ok(selected)
}

pub(super) fn name(path: &Path) -> Result<String, String> {
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no UTF-8 resource name", path.display()))?;
    let stem = stem.strip_suffix(".agent.md").unwrap_or(stem);
    let stem = stem.strip_suffix(".md").unwrap_or(stem);
    if bureau_plugin::is_plugin_reference(&format!("/{stem}:resource")) {
        return Ok(stem.to_owned());
    }
    Err(format!("unsupported resource name `{stem}`"))
}

pub(super) fn repository(worktree: &Path) -> Result<(), String> {
    for path in [
        ".mcp.json",
        ".github/hooks",
        ".github/copilot/hooks",
        ".claude/hooks",
    ] {
        if exists(&worktree.join(path))? {
            return Err(format!(
                "factory context refuses repository hooks/MCP at `{path}`; only audited pinned plugins and Bureau's own MCP mapping are supported"
            ));
        }
    }
    Ok(())
}
