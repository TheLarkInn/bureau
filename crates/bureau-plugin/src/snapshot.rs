//! Durable per-run plugin snapshots and resume validation.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use ring::rand::SecureRandom as _;
use serde_json::Value;

use super::tree::Tree;
use super::{Error, PluginSource, Resolved, json, paths};

fn present(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::io("inspect", path, error)),
    }
}

fn string_field<'a>(value: &'a Value, key: &str, root: &Path) -> Result<&'a str, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(&root.join("plugin.json"), format!("`{key}` is required")))
}

fn manifest(tree: &Tree, root: &Path) -> Result<Value, Error> {
    let path = root.join("plugin.json");
    let file = tree
        .file(Path::new("plugin.json"))
        .ok_or_else(|| Error::invalid(&path, "plugin.json is required"))?;
    json::parse(&file.bytes, &path)
}

fn sync_dir(path: &Path) -> Result<(), Error> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| Error::io("sync directory", path, error))
}

fn sync_tree(directory: &Path) -> Result<(), Error> {
    for entry in
        fs::read_dir(directory).map_err(|error| Error::io("read snapshot", directory, error))?
    {
        let path = entry
            .map_err(|error| Error::io("read snapshot", directory, error))?
            .path();
        if path.is_dir() {
            sync_tree(&path)?;
        } else {
            fs::File::open(&path)
                .and_then(|file| file.sync_all())
                .map_err(|error| Error::io("sync snapshot", &path, error))?;
        }
    }
    sync_dir(directory)
}

fn remove_stale(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io("remove stale metadata", path, error)),
    }
}

fn write_atomic(temporary: &Path, destination: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(|error| Error::io("create metadata", temporary, error))?;
    file.write_all(bytes)
        .map_err(|error| Error::io("write metadata", temporary, error))?;
    file.sync_all()
        .map_err(|error| Error::io("sync metadata", temporary, error))?;
    fs::rename(temporary, destination)
        .map_err(|error| Error::io("commit metadata", destination, error))
}

fn write_metadata(path: &Path, source: &PluginSource) -> Result<(), Error> {
    let value = serde_json::to_value(source).map_err(|error| Error::invalid(path, error))?;
    let bytes = json::format(&value, path)?;
    let temporary = path.with_extension("json.tmp");
    remove_stale(&temporary)?;
    let result = write_atomic(&temporary, path, &bytes);
    if result.is_err() {
        let _removed = fs::remove_file(&temporary);
    }
    result
}

fn write_stage(stage: &Path, tree: &Tree, source: &PluginSource) -> Result<(), Error> {
    tree.write(&stage.join("tree"))?;
    write_metadata(&stage.join("source.json"), source)?;
    sync_tree(stage)
}

fn stage_path(parent: &Path, plugin: &str) -> Result<PathBuf, Error> {
    let mut bytes = [0_u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::invalid(parent, "random source unavailable"))?;
    Ok(parent.join(format!(
        ".{plugin}-copy-{:032x}",
        u128::from_le_bytes(bytes)
    )))
}

struct SnapshotPaths {
    parent: PathBuf,
    container: PathBuf,
    root: PathBuf,
    metadata: PathBuf,
    plugin: String,
}

impl SnapshotPaths {
    fn new(run_dir: &Path, plugin: &str) -> Self {
        let parent = run_dir.join("plugins");
        let container = parent.join(plugin);
        Self {
            root: container.join("tree"),
            metadata: container.join("source.json"),
            parent,
            container,
            plugin: plugin.to_owned(),
        }
    }
}

fn commit_snapshot(paths: &SnapshotPaths, tree: &Tree, source: &PluginSource) -> Result<(), Error> {
    let stage = stage_path(&paths.parent, &paths.plugin)?;
    fs::create_dir(&stage).map_err(|error| Error::io("create snapshot stage", &stage, error))?;
    let result = write_stage(&stage, tree, source);
    if let Err(error) = result {
        let _removed = fs::remove_dir_all(&stage);
        return Err(error);
    }
    match fs::rename(&stage, &paths.container) {
        Ok(()) => sync_dir(&paths.parent),
        Err(error) => {
            let _removed = fs::remove_dir_all(&stage);
            Err(Error::io("commit snapshot", &paths.container, error))
        }
    }
}

fn ensure_absent(paths: &SnapshotPaths) -> Result<(), Error> {
    if present(&paths.container)? {
        return Err(Error::invalid(
            &paths.container,
            "durable plugin snapshot appeared during resolution",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct Snapshot {
    pub root: PathBuf,
    pub source: PluginSource,
    pub tree: Tree,
}

fn metadata(
    plugin: &str,
    description: &str,
    tree: &Tree,
    root: &Path,
) -> Result<PluginSource, Error> {
    let manifest = manifest(tree, root)?;
    let name = string_field(&manifest, "name", root)?;
    let version = string_field(&manifest, "version", root)?;
    if name != plugin {
        return Err(Error::invalid(
            root,
            "plugin.json name does not match the reference",
        ));
    }
    Ok(PluginSource {
        name: name.to_owned(),
        source: description.to_owned(),
        version: version.to_owned(),
        digest: tree.digest()?,
        origin: None,
    })
}

fn validate_source(source: &PluginSource, tree: &Tree, root: &Path) -> Result<(), Error> {
    let mut current = metadata(&source.name, &source.source, tree, root)?;
    current.origin.clone_from(&source.origin);
    if current.version == source.version && current.digest == source.digest {
        return Ok(());
    }
    Err(Error::invalid(
        root,
        "durable plugin snapshot metadata or content changed",
    ))
}

fn validated(paths: SnapshotPaths) -> Result<Snapshot, Error> {
    let tree = Tree::inspect(&paths.root)?;
    let value = json::read(&paths.metadata)?;
    let source: PluginSource =
        serde_json::from_value(value).map_err(|error| Error::invalid(&paths.metadata, error))?;
    if source.name != paths.plugin {
        return Err(Error::invalid(
            &paths.metadata,
            "durable plugin snapshot name does not match its directory",
        ));
    }
    validate_source(&source, &tree, &paths.root)?;
    Ok(Snapshot {
        root: paths.root,
        source,
        tree,
    })
}

pub fn load(run_dir: &Path, plugin: &str) -> Result<Option<Snapshot>, Error> {
    let paths = SnapshotPaths::new(run_dir, plugin);
    if !present(&paths.container)? {
        return Ok(None);
    }
    validated(paths).map(Some)
}

pub fn create(run_dir: &Path, plugin: &str, resolved: &Resolved) -> Result<Snapshot, Error> {
    let paths = SnapshotPaths::new(run_dir, plugin);
    paths::ensure_plain_dir(&paths.parent)?;
    sync_dir(run_dir)?;
    ensure_absent(&paths)?;
    let tree = Tree::inspect(&resolved.path)?;
    let mut source = metadata(plugin, &resolved.description, &tree, &resolved.path)?;
    source.origin = Some(
        fs::canonicalize(&resolved.path)
            .map_err(|error| Error::io("resolve plugin source", &resolved.path, error))?,
    );
    commit_snapshot(&paths, &tree, &source)?;
    validated(paths)
}
