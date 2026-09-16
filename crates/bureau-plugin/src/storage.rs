//! Shared directory durability for pinned code trees.

use std::fs;
use std::path::{Path, PathBuf};

use ring::rand::SecureRandom as _;

use super::Error;

#[cfg(test)]
mod tests;

pub fn sync_dir(path: &Path) -> Result<(), Error> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| Error::io("sync directory", path, error))
}

pub fn sync_tree(directory: &Path) -> Result<(), Error> {
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

pub fn stage_path(parent: &Path, name: &str) -> Result<PathBuf, Error> {
    let mut bytes = [0_u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::invalid(parent, "random source unavailable"))?;
    Ok(parent.join(format!(".{name}-copy-{:032x}", u128::from_le_bytes(bytes))))
}

#[cfg(target_os = "linux")]
pub fn publish_new(parent: &Path, stage: &Path, destination: &Path) -> Result<(), Error> {
    let directory = fs::File::open(parent).map_err(|error| Error::io("open", parent, error))?;
    let source = stage
        .file_name()
        .ok_or_else(|| Error::invalid(stage, "snapshot stage has no name"))?;
    let target = destination
        .file_name()
        .ok_or_else(|| Error::invalid(destination, "snapshot has no name"))?;
    rustix::fs::renameat_with(
        &directory,
        source,
        &directory,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| Error::io("publish new code snapshot", destination, error.into()))
}

#[cfg(not(target_os = "linux"))]
pub fn publish_new(_parent: &Path, _stage: &Path, destination: &Path) -> Result<(), Error> {
    Err(Error::io(
        "publish new code snapshot",
        destination,
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "local factory snapshots require Linux atomic no-replace directory publication",
        ),
    ))
}
