//! Symlink-free plugin tree inspection, copying, and deterministic hashing.

use std::fmt::Write as _;
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use super::Error;

#[derive(Debug)]
pub struct TreeFile {
    pub relative: PathBuf,
    pub bytes: Vec<u8>,
    pub permissions: Permissions,
}

fn relative(root: &Path, path: &Path) -> Result<PathBuf, Error> {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|error| Error::invalid(path, error))
}

fn check_root(root: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(root).map_err(|error| Error::io("inspect", root, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::invalid(
            root,
            "plugin root must be a directory without symlinks",
        ));
    }
    Ok(())
}

fn collect_file(
    path: &Path,
    relative: PathBuf,
    permissions: Permissions,
    tree: &mut Tree,
) -> Result<(), Error> {
    let metadata = fs::metadata(path).map_err(|error| Error::io("inspect", path, error))?;
    if !metadata.is_file() {
        return Err(Error::invalid(
            path,
            "plugin trees may contain only files and directories",
        ));
    }
    let bytes = fs::read(path).map_err(|error| Error::io("read", path, error))?;
    tree.files.push(TreeFile {
        relative,
        bytes,
        permissions,
    });
    Ok(())
}

fn collect(root: &Path, directory: &Path, tree: &mut Tree) -> Result<(), Error> {
    let entries =
        fs::read_dir(directory).map_err(|error| Error::io("read directory", directory, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| Error::io("read directory", directory, error))?;
        collect_entry(root, &entry.path(), tree)?;
    }
    Ok(())
}

fn collect_entry(root: &Path, path: &Path, tree: &mut Tree) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|error| Error::io("inspect", path, error))?;
    let relative = relative(root, path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::invalid(
            path,
            "plugin trees may not contain symlinks",
        ));
    }
    if metadata.is_dir() {
        tree.directories.push(relative);
        return collect(root, path, tree);
    }
    collect_file(path, relative, metadata.permissions(), tree)
}

fn create_dir(path: &Path) -> Result<(), Error> {
    fs::create_dir(path).map_err(|error| Error::io("create directory", path, error))
}

fn write_file(destination: &Path, file: &TreeFile) -> Result<(), Error> {
    let path = destination.join(&file.relative);
    fs::write(&path, &file.bytes).map_err(|error| Error::io("write", &path, error))?;
    fs::set_permissions(&path, file.permissions.clone())
        .map_err(|error| Error::io("set permissions", &path, error))
}

struct DigestEntry<'a> {
    path: String,
    kind: u8,
    mode: u32,
    bytes: &'a [u8],
}

fn digest_path(path: &Path) -> Result<String, Error> {
    let Some(path) = path.to_str() else {
        return Err(Error::invalid(path, "plugin paths must be valid UTF-8"));
    };
    Ok(path.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn digest_entries(tree: &Tree) -> Result<Vec<DigestEntry<'_>>, Error> {
    let mut entries = Vec::with_capacity(tree.directories.len() + tree.files.len());
    for path in &tree.directories {
        entries.push(DigestEntry {
            path: digest_path(path)?,
            kind: b'D',
            mode: 0,
            bytes: &[],
        });
    }
    for file in &tree.files {
        entries.push(DigestEntry {
            path: digest_path(&file.relative)?,
            kind: b'F',
            mode: file.permissions.mode() & 0o7777,
            bytes: &file.bytes,
        });
    }
    Ok(entries)
}

struct Digest(ring::digest::Context);

impl Digest {
    fn new() -> Self {
        Self(ring::digest::Context::new(&ring::digest::SHA256))
    }

    fn add_entry(&mut self, entry: &DigestEntry<'_>) {
        self.add(&[entry.kind]);
        self.add_number(entry.path.len());
        self.add(entry.path.as_bytes());
        self.add(&entry.mode.to_le_bytes());
        self.add_number(entry.bytes.len());
        self.add(entry.bytes);
    }

    fn add_number(&mut self, value: usize) {
        self.add(&(value as u64).to_le_bytes());
    }

    fn add(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn finish(self) -> String {
        let mut value = "tree-sha256:".to_owned();
        for byte in self.0.finish().as_ref() {
            let _ = write!(value, "{byte:02x}");
        }
        value
    }
}

#[derive(Debug)]
pub struct Tree {
    pub directories: Vec<PathBuf>,
    pub files: Vec<TreeFile>,
}

impl Tree {
    pub fn inspect(root: &Path) -> Result<Self, Error> {
        check_root(root)?;
        let mut tree = Self {
            directories: Vec::new(),
            files: Vec::new(),
        };
        collect(root, root, &mut tree)?;
        tree.directories.sort();
        tree.files
            .sort_by(|left, right| left.relative.cmp(&right.relative));
        Ok(tree)
    }

    pub fn write(&self, destination: &Path) -> Result<(), Error> {
        create_dir(destination)?;
        for relative in &self.directories {
            create_dir(&destination.join(relative))?;
        }
        for file in &self.files {
            write_file(destination, file)?;
        }
        Ok(())
    }

    pub fn file(&self, relative: &Path) -> Option<&TreeFile> {
        self.files.iter().find(|file| file.relative == relative)
    }

    /// SHA-256 over sorted paths, file bytes, and normalized permissions.
    pub fn digest(&self) -> Result<String, Error> {
        let mut entries = digest_entries(self)?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let mut digest = Digest::new();
        for entry in entries {
            digest.add_entry(&entry);
        }
        Ok(digest.finish())
    }
}
