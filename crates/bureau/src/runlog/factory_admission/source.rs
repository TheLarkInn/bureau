use std::fs::{self, File, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use super::super::EVENTS_FILE;

mod capture;
#[cfg(test)]
mod tests;

type Identity = (u64, u64);

fn identity(metadata: &Metadata) -> Identity {
    (metadata.dev(), metadata.ino())
}

#[derive(PartialEq, Eq)]
struct Directory {
    path: PathBuf,
    identity: Identity,
}

fn directory(entry: &fs::DirEntry) -> io::Result<Option<Directory>> {
    let metadata = fs::symlink_metadata(entry.path())?;
    if metadata.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "run directory is symlinked",
        ));
    }
    Ok(metadata.is_dir().then(|| Directory {
        path: entry.path(),
        identity: identity(&metadata),
    }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Version {
    pub(super) identity: Identity,
    pub(super) len: u64,
    pub(super) digest: [u8; 32],
}

pub(super) struct Appended {
    pub(super) base: Version,
    pub(super) digest: [u8; 32],
    pub(super) bytes: Option<Vec<u8>>,
}

struct Contents {
    version: Version,
    has_newline: bool,
    ended: bool,
    appended: Option<Appended>,
}

pub(super) struct RunSource {
    directory: Directory,
    contents: Option<Contents>,
}

impl RunSource {
    fn read(
        directory: Directory,
        previous: Option<&Self>,
        remaining: &mut usize,
    ) -> io::Result<Self> {
        let previous = previous
            .filter(|source| source.directory == directory)
            .and_then(|source| source.contents.as_ref())
            .filter(|contents| contents.ended);
        let contents = capture::read(&directory.path.join(EVENTS_FILE), previous, remaining)?;
        Ok(Self {
            directory,
            contents,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.directory.path
    }

    pub(super) fn version(&self) -> Option<Version> {
        self.contents.as_ref().map(|contents| contents.version)
    }

    pub(super) fn unpublished(&self) -> bool {
        self.contents
            .as_ref()
            .is_none_or(|contents| !contents.has_newline)
    }

    pub(super) fn same(&self, other: &Self) -> bool {
        self.directory == other.directory && self.version() == other.version()
    }

    pub(super) fn appended<'a>(&self, next: &'a Self) -> Option<&'a Appended> {
        let before = self.contents.as_ref()?;
        let after = next.contents.as_ref()?;
        let appended = after.appended.as_ref()?;
        if self.directory != next.directory || !before.ended {
            return None;
        }
        if before.version.identity != after.version.identity {
            return None;
        }
        (before.version == appended.base).then_some(appended)
    }

    pub(super) fn open(&self) -> io::Result<Option<File>> {
        let file = File::open(self.path().join(EVENTS_FILE))?;
        let current = identity(&file.metadata()?);
        Ok(self
            .version()
            .is_some_and(|version| version.identity == current)
            .then_some(file))
    }

    fn release_tail(&mut self) {
        if let Some(appended) = self
            .contents
            .as_mut()
            .and_then(|contents| contents.appended.as_mut())
        {
            appended.bytes = None;
        }
    }
}

fn entries(root: &Path) -> io::Result<Vec<Directory>> {
    let mut directories = Vec::new();
    for entry in fs::read_dir(root)? {
        if let Some(directory) = directory(&entry?)? {
            directories.push(directory);
        }
    }
    directories.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(directories)
}

fn captured_runs(root: &Path, previous: Option<&[RunSource]>) -> io::Result<Vec<RunSource>> {
    let mut remaining = super::FENCED_OUTPUT_BYTES;
    let mut runs = Vec::new();
    for directory in entries(root)? {
        let prior = previous.and_then(|runs| {
            runs.binary_search_by(|run| run.path().cmp(&directory.path))
                .ok()
                .map(|index| &runs[index])
        });
        runs.push(RunSource::read(directory, prior, &mut remaining)?);
    }
    Ok(runs)
}

/// Streamed byte fingerprints captured under the fence; not a durable authority or cache.
pub struct FactorySource {
    identity: Option<Identity>,
    pub(super) runs: Vec<RunSource>,
}

impl FactorySource {
    pub(crate) fn capture(root: &Path, previous: Option<&Self>) -> io::Result<Self> {
        let metadata = match fs::metadata(root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    identity: None,
                    runs: Vec::new(),
                });
            }
            Err(error) => return Err(error),
        };
        Ok(Self {
            identity: Some(identity(&metadata)),
            runs: captured_runs(root, previous.map(|source| source.runs.as_slice()))?,
        })
    }

    pub(crate) fn read(root: &Path) -> io::Result<Self> {
        Self::capture(root, None)
    }

    pub(super) fn same_root(&self, other: &Self) -> bool {
        self.identity == other.identity && self.runs.len() == other.runs.len()
    }

    pub(super) fn release_tails(&mut self) {
        for run in &mut self.runs {
            run.release_tail();
        }
    }

    #[cfg(test)]
    fn buffered_bytes(&self) -> usize {
        self.runs
            .iter()
            .filter_map(|source| source.contents.as_ref())
            .filter_map(|contents| contents.appended.as_ref())
            .filter_map(|appended| appended.bytes.as_ref())
            .map(Vec::capacity)
            .sum()
    }
}
