//! Temporary worktree-local plugin and agent materialization.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::guard::Guard;
use super::settings::{self, Marketplace, Settings};
use super::snapshot::Snapshot;
use super::{Error, catalog, json, paths};

static NEXT_ACTIVATION: AtomicU64 = AtomicU64::new(0);

pub fn apply(
    snapshot: &Snapshot,
    agent: &str,
    worktree: &Path,
    settings: &Settings,
) -> Result<Guard, Error> {
    let selection = select(worktree, settings)?;
    let mut guard = Guard::new();
    let result = setup(snapshot, agent, worktree, settings, &selection, &mut guard);
    match result {
        Ok(()) => Ok(guard),
        Err(error) => rollback(guard, error),
    }
}

pub fn apply_direct(bytes: &[u8], agent: &str, worktree: &Path) -> Result<Guard, Error> {
    let mut guard = Guard::new();
    let result = write_direct(bytes, agent, worktree, &mut guard);
    match result {
        Ok(()) => Ok(guard),
        Err(error) => rollback(guard, error),
    }
}

fn setup(
    snapshot: &Snapshot,
    agent: &str,
    worktree: &Path,
    settings: &Settings,
    selection: &Selection,
    guard: &mut Guard,
) -> Result<(), Error> {
    guard.create_dir_all(&selection.root)?;
    guard.create_dir_all(parent(&selection.catalog)?)?;
    let temporary = temporary_path(&selection.root);
    guard.create_temporary_dir(&temporary.base)?;
    copy_plugin(snapshot, &temporary, guard)?;
    write_catalog(snapshot, selection, &temporary, guard)?;
    write_settings(snapshot, worktree, settings, selection, guard)?;
    write_agents(snapshot, agent, worktree, guard)
}

fn rollback(mut guard: Guard, error: Error) -> Result<Guard, Error> {
    match guard.restore() {
        Ok(()) => Err(error),
        Err(restore) => Err(restore),
    }
}

fn copy_plugin(
    snapshot: &Snapshot,
    temporary: &TemporaryPath,
    guard: &mut Guard,
) -> Result<(), Error> {
    guard.create_dir_all(&temporary.plugin_root)?;
    for directory in &snapshot.tree.directories {
        guard.create_dir_all(&temporary.plugin_root.join(directory))?;
    }
    for file in &snapshot.tree.files {
        copy_file(&temporary.plugin_root, file, guard)?;
    }
    Ok(())
}

fn copy_file(root: &Path, file: &super::tree::TreeFile, guard: &mut Guard) -> Result<(), Error> {
    let path = root.join(&file.relative);
    guard.write(&path, &file.bytes)?;
    Guard::set_permissions(&path, file.permissions.clone())
}

fn write_catalog(
    snapshot: &Snapshot,
    selection: &Selection,
    temporary: &TemporaryPath,
    guard: &mut Guard,
) -> Result<(), Error> {
    let mut value = selection.catalog_value.clone();
    catalog::inject(
        &mut value,
        &selection.catalog,
        &snapshot.source.name,
        &snapshot.source.version,
        &temporary.catalog_source,
    )?;
    let bytes = json::format(&value, &selection.catalog)?;
    guard.write(&selection.catalog, &bytes)
}

fn write_settings(
    snapshot: &Snapshot,
    worktree: &Path,
    settings: &Settings,
    selection: &Selection,
    guard: &mut Guard,
) -> Result<(), Error> {
    let path = worktree.join(settings::SETTINGS_PATH);
    guard.create_dir_all(parent(&path)?)?;
    let mut value = settings.value.clone();
    if selection.register {
        settings::register_local(&mut value, &selection.name, ".ai")?;
    }
    settings::enable(&mut value, &snapshot.source.name, &selection.name)?;
    let bytes = json::format(&value, &path)?;
    guard.write(&path, &bytes)
}

fn write_agents(
    snapshot: &Snapshot,
    agent: &str,
    worktree: &Path,
    guard: &mut Guard,
) -> Result<(), Error> {
    let relative = PathBuf::from(format!("agents/{agent}.agent.md"));
    let source_path = snapshot.root.join(&relative);
    let file = snapshot
        .tree
        .file(&relative)
        .ok_or_else(|| Error::invalid(&source_path, "requested agent file is missing"))?;
    let destinations = agent_destinations(worktree, agent);
    for destination in destinations {
        guard.create_dir_all(parent(&destination)?)?;
        guard.write(&destination, &file.bytes)?;
    }
    Ok(())
}

fn write_direct(
    bytes: &[u8],
    agent: &str,
    worktree: &Path,
    guard: &mut Guard,
) -> Result<(), Error> {
    for destination in agent_destinations(worktree, agent) {
        guard.create_dir_all(parent(&destination)?)?;
        guard.write(&destination, bytes)?;
    }
    Ok(())
}

fn agent_destinations(worktree: &Path, agent: &str) -> [PathBuf; 2] {
    [
        worktree
            .join(".github/agents")
            .join(format!("{agent}.agent.md")),
        worktree.join(".claude/agents").join(format!("{agent}.md")),
    ]
}

fn select(worktree: &Path, settings: &Settings) -> Result<Selection, Error> {
    let marketplaces = settings.local_marketplaces(worktree)?;
    marketplaces
        .into_iter()
        .next()
        .map_or_else(|| Selection::created(worktree), Selection::existing)
}

#[derive(Debug)]
struct Selection {
    name: String,
    root: PathBuf,
    catalog: PathBuf,
    catalog_value: Value,
    register: bool,
}

impl Selection {
    fn existing(marketplace: Marketplace) -> Result<Self, Error> {
        let catalog_value = json::read(&marketplace.catalog)?;
        Ok(Self {
            name: marketplace.name,
            root: marketplace.root,
            catalog: marketplace.catalog,
            catalog_value,
            register: false,
        })
    }

    fn created(worktree: &Path) -> Result<Self, Error> {
        let root = paths::contained_path(worktree, Path::new(".ai"))?;
        Ok(Self {
            catalog: root.join("marketplace.json"),
            root,
            name: "repo-plugins".to_owned(),
            catalog_value: catalog::new_marketplace(),
            register: true,
        })
    }
}

struct TemporaryPath {
    base: PathBuf,
    plugin_root: PathBuf,
    catalog_source: String,
}

fn temporary_path(root: &Path) -> TemporaryPath {
    loop {
        let next = NEXT_ACTIVATION.fetch_add(1, Ordering::Relaxed);
        let name = format!(".bureau-activation-{}-{next}", std::process::id());
        let base = root.join(&name);
        if fs::symlink_metadata(&base).is_err() {
            return TemporaryPath {
                base: base.clone(),
                plugin_root: base.join("plugin"),
                catalog_source: format!("{name}/plugin"),
            };
        }
    }
}

fn parent(path: &Path) -> Result<&Path, Error> {
    path.parent()
        .ok_or_else(|| Error::invalid(path, "activation path has no parent"))
}
