//! Pinned non-plugin code trees using the plugin snapshot's digest and durability.

use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Component, Path, PathBuf};

use super::storage::{publish_new, stage_path, sync_dir, sync_tree};
use super::tree::Tree;
use super::{Error, paths};

/// Computes the established digest over paths, bytes, and file permissions.
///
/// # Errors
/// Rejects symlinks, non-files, invalid paths, and unreadable content.
pub fn tree_digest(directory: &Path) -> Result<String, Error> {
    Tree::inspect(directory)?.digest()
}

fn checked_tree(directory: &Path, expected: &str) -> Result<Tree, Error> {
    let tree = Tree::inspect(directory)?;
    let actual = tree.digest()?;
    if actual != expected {
        return Err(Error::invalid(
            directory,
            format!("code digest mismatch: expected {expected}, observed {actual}"),
        ));
    }
    Ok(tree)
}

fn present(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::io("inspect snapshot", path, error)),
    }
}

fn discard_stage(stage: &Path, original: Error) -> Error {
    match fs::remove_dir_all(stage) {
        Ok(()) => original,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => original,
        Err(error) => Error::invalid(
            stage,
            format!("{original}; cleaning incomplete code snapshot failed: {error}"),
        ),
    }
}

fn write_stage(stage: &Path, tree: &Tree) -> Result<(), Error> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(stage)
        .map_err(|error| Error::io("create code snapshot", stage, error))?;
    tree.write_contents(stage)?;
    sync_tree(stage)
}

fn commit_tree(destination: &Path, tree: &Tree) -> Result<(), Error> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::invalid(destination, "code snapshot has no parent"))?;
    paths::ensure_plain_dir(parent)?;
    let stage = stage_path(parent, "code")?;
    if let Err(error) = write_stage(&stage, tree) {
        return Err(discard_stage(&stage, error));
    }
    if let Err(error) = publish_new(parent, &stage, destination) {
        return Err(discard_stage(&stage, error));
    }
    sync_dir(parent)
}

/// Exact per-run code material, independently verifiable after restart.
#[derive(Debug, Clone)]
pub struct TreeSnapshot {
    directory: PathBuf,
    digest: String,
}

impl TreeSnapshot {
    /// Requires a run-owned snapshot root outside every agent-writable worktree path.
    ///
    /// # Errors
    /// Rejects lexical or symlink-mediated containment and unsafe path ancestors.
    pub fn require_outside(directory: &Path, worktree: &Path) -> Result<(), Error> {
        paths::ensure_outside(directory, worktree)
    }

    /// Reopens existing material without resolving or replacing its source.
    ///
    /// # Errors
    /// Rejects missing, changed, or symlinked snapshot content.
    pub fn open(directory: &Path, expected: &str) -> Result<Self, Error> {
        checked_tree(directory, expected)?;
        let directory = fs::canonicalize(directory)
            .map_err(|error| Error::io("resolve code snapshot", directory, error))?;
        Ok(Self {
            directory,
            digest: expected.to_owned(),
        })
    }

    /// Publishes approved code atomically into a private, previously absent directory.
    /// Existing material is verified, never overwritten or reconstructed.
    ///
    /// # Errors
    /// Rejects wrong source digests, unsafe paths, corrupt existing snapshots, or I/O failures.
    pub fn pin(source: &Path, destination: &Path, expected: &str) -> Result<Self, Error> {
        if present(destination)? {
            return Self::open(destination, expected);
        }
        paths::ensure_outside(destination, source)?;
        let tree = checked_tree(source, expected)?;
        commit_tree(destination, &tree)?;
        Self::open(destination, expected)
    }

    /// Checks the complete material again before using it.
    ///
    /// # Errors
    /// Rejects changed, missing, or unsafe snapshot content.
    pub fn verify(&self) -> Result<(), Error> {
        checked_tree(&self.directory, &self.digest).map(|_| ())
    }

    /// Reads a file from the same captured bytes whose complete digest was verified.
    ///
    /// # Errors
    /// Rejects missing files, traversal, unsafe trees, and digest mismatches.
    pub fn read(&self, relative: &Path) -> Result<Vec<u8>, Error> {
        paths::contained_path(&self.directory, relative)?;
        let normalized: PathBuf = relative
            .components()
            .filter(|component| *component != Component::CurDir)
            .collect();
        let tree = checked_tree(&self.directory, &self.digest)?;
        tree.file(&normalized)
            .map(|file| file.bytes.clone())
            .ok_or_else(|| Error::invalid(relative, "file is absent from pinned code"))
    }

    /// Resolves an existing entry inside verified code material.
    ///
    /// # Errors
    /// Rejects traversal, missing paths, unsafe trees, and digest mismatches.
    pub fn path(&self, relative: &Path) -> Result<PathBuf, Error> {
        self.verify()?;
        paths::contained_existing(&self.directory, relative)?
            .map(|path| path.components().collect())
            .ok_or_else(|| Error::invalid(relative, "path is absent from pinned code"))
    }

    /// The private, durable snapshot directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The approved content identity, including file permissions.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}
