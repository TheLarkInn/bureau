//! Exact restoration guard for temporary worktree activation files.

mod durable;

use std::fs::{self, Permissions};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use super::{Error, restoration};

fn validate_directory(path: &Path, metadata: &fs::Metadata) -> Result<(), Error> {
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        return Ok(());
    }
    Err(Error::invalid(
        path,
        "activation directories may not be symlinks",
    ))
}

fn verify_ancestors(path: &Path) -> Result<(), Error> {
    let absolute = std::path::absolute(path).map_err(|error| Error::io("resolve", path, error))?;
    let mut ancestors: Vec<&Path> = absolute.ancestors().skip(1).collect();
    ancestors.reverse();
    for ancestor in ancestors {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(Error::io("inspect ancestor", ancestor, error)),
        };
        validate_directory(ancestor, &metadata)?;
    }
    Ok(())
}

fn remove_current(path: &Path) -> Result<(), Error> {
    verify_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|error| Error::io("remove", path, error))
        }
        Ok(_) => fs::remove_file(path).map_err(|error| Error::io("remove", path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io("inspect", path, error)),
    }
}

fn remove_directory(path: &Path) -> Result<(), Error> {
    verify_ancestors(path)?;
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(error) => Err(Error::io("remove directory", path, error)),
    }
}

fn remove_temporary(path: &Path) -> Result<(), Error> {
    verify_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)
                .map_err(|error| Error::io("remove temporary directory", path, error))
        }

        Ok(_) => {
            fs::remove_file(path).map_err(|error| Error::io("remove temporary path", path, error))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io("inspect", path, error)),
    }
}

fn restoration_result(conflicts: Vec<PathBuf>, failures: Vec<String>) -> Result<(), Error> {
    if !conflicts.is_empty() {
        return Err(Error::Conflict {
            paths: conflicts,
            restore_failures: failures,
        });
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Error::Restore(failures))
    }
}

#[derive(Debug)]
pub struct Original {
    pub(super) bytes: Vec<u8>,
    pub(super) permissions: Permissions,
}

#[derive(Debug)]
pub struct SavedFile {
    pub(super) path: PathBuf,
    pub(super) original: Option<Original>,
    pub(super) injected: Vec<u8>,
}

impl SavedFile {
    fn matches_injected(&self) -> bool {
        verify_ancestors(&self.path).is_ok()
            && fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && fs::read(&self.path).is_ok_and(|bytes| bytes == self.injected)
            })
    }

    fn restore(&self) -> Result<(), Error> {
        verify_ancestors(&self.path)?;
        remove_current(&self.path)?;
        let Some(original) = &self.original else {
            return Ok(());
        };
        fs::write(&self.path, &original.bytes)
            .map_err(|error| Error::io("restore", &self.path, error))?;
        fs::set_permissions(&self.path, original.permissions.clone())
            .map_err(|error| Error::io("restore permissions", &self.path, error))
    }
}

fn read_original(path: &Path) -> Result<Option<Original>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::io("inspect", path, error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.nlink() > 1 {
        return Err(Error::invalid(
            path,
            "activation path must be a regular file with one link",
        ));
    }
    let bytes = fs::read(path).map_err(|error| Error::io("read", path, error))?;
    Ok(Some(Original {
        bytes,
        permissions: metadata.permissions(),
    }))
}

fn missing_directories(path: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                validate_directory(current, &metadata)?;
                verify_ancestors(&current.join(".bureau-ancestor-check"))?;
                return Ok(missing);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                current = current
                    .parent()
                    .ok_or_else(|| Error::invalid(path, "directory has no existing parent"))?;
            }
            Err(error) => return Err(Error::io("inspect", current, error)),
        }
    }
}

#[derive(Debug)]
pub struct Guard {
    files: Vec<SavedFile>,
    directories: Vec<PathBuf>,
    temporary_roots: Vec<PathBuf>,
    restoration: Option<restoration::Record>,
    active: bool,
}

impl Guard {
    pub fn create_dir_all(&mut self, path: &Path) -> Result<(), Error> {
        let mut missing = missing_directories(path)?;
        missing.reverse();
        for directory in missing {
            self.directories.push(directory.clone());
            self.persist()?;
            fs::create_dir(&directory)
                .map_err(|error| Error::io("create directory", &directory, error))?;
        }
        Ok(())
    }

    pub fn create_temporary_dir(&mut self, path: &Path) -> Result<(), Error> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io("inspect", path, error)),
            Ok(_) => {
                return Err(Error::invalid(
                    path,
                    "temporary activation path already exists",
                ));
            }
        }
        self.create_dir_all(path)?;
        self.temporary_roots.push(path.to_path_buf());
        self.persist()?;
        Ok(())
    }

    pub fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), Error> {
        let index = self.saved_index(path)?;
        self.files[index].injected = bytes.to_vec();
        self.persist()?;
        verify_ancestors(path)?;
        fs::write(path, bytes).map_err(|error| Error::io("write", path, error))?;
        Ok(())
    }

    pub fn set_permissions(path: &Path, permissions: Permissions) -> Result<(), Error> {
        fs::set_permissions(path, permissions)
            .map_err(|error| Error::io("set permissions", path, error))
    }

    pub fn restore(&mut self) -> Result<(), Error> {
        if !self.active {
            return Ok(());
        }
        let conflicts = self.conflicts();
        let mut failures = self.restore_files();
        failures.extend(self.remove_temporary_roots());
        failures.extend(self.remove_directories());
        if failures.is_empty() {
            if let Some(restoration) = &self.restoration {
                if let Err(error) = restoration.remove() {
                    failures.push(error.to_string());
                }
            }
        }
        self.active = false;
        restoration_result(conflicts, failures)
    }

    fn saved_index(&mut self, path: &Path) -> Result<usize, Error> {
        if let Some(index) = self.files.iter().position(|file| file.path == path) {
            return Ok(index);
        }
        verify_ancestors(path)?;
        let original = read_original(path)?;
        self.files.push(SavedFile {
            path: path.to_path_buf(),
            original,
            injected: Vec::new(),
        });
        Ok(self.files.len() - 1)
    }

    fn conflicts(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|file| !file.matches_injected())
            .map(|file| file.path.clone())
            .collect()
    }

    fn restore_files(&self) -> Vec<String> {
        self.files
            .iter()
            .rev()
            .filter_map(|file| file.restore().err())
            .map(|error| error.to_string())
            .collect()
    }

    fn remove_directories(&self) -> Vec<String> {
        self.directories
            .iter()
            .rev()
            .filter_map(|path| remove_directory(path).err())
            .map(|error| error.to_string())
            .collect()
    }

    fn remove_temporary_roots(&self) -> Vec<String> {
        self.temporary_roots
            .iter()
            .rev()
            .filter_map(|path| remove_temporary(path).err())
            .map(|error| error.to_string())
            .collect()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _restored = self.restore();
    }
}
