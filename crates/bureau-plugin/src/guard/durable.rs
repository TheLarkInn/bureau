use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use super::{Guard, Original, SavedFile};
use crate::Error;
use crate::restoration::{self, RecordedFile, RecordedOriginal};

fn recorded_file(file: &SavedFile) -> RecordedFile {
    RecordedFile {
        path: file.path.clone(),
        original: file.original.as_ref().map(|original| RecordedOriginal {
            bytes: original.bytes.clone(),
            mode: original.permissions.mode(),
        }),
        injected: file.injected.clone(),
    }
}

fn saved_file(file: RecordedFile) -> SavedFile {
    SavedFile {
        path: file.path,
        original: file.original.map(|original| Original {
            bytes: original.bytes,
            permissions: Permissions::from_mode(original.mode),
        }),
        injected: file.injected,
    }
}

impl Guard {
    pub(crate) fn durable(
        run_dir: &Path,
        worktree: &Path,
        plugin: &str,
        version: &str,
        source: Option<&Path>,
    ) -> Result<Self, Error> {
        let restoration = restoration::Record::create(run_dir, worktree, plugin, version, source)?;
        Ok(Self {
            files: Vec::new(),
            directories: Vec::new(),
            temporary_roots: Vec::new(),
            restoration: Some(restoration),
            active: true,
        })
    }

    pub(crate) fn from_loaded(loaded: restoration::Loaded) -> Self {
        Self {
            files: loaded.files.into_iter().map(saved_file).collect(),
            directories: loaded.directories,
            temporary_roots: loaded.temporary_roots,
            restoration: Some(loaded.record),
            active: true,
        }
    }

    pub(super) fn persist(&self) -> Result<(), Error> {
        self.restoration.as_ref().map_or(Ok(()), |restoration| {
            let files: Vec<RecordedFile> = self.files.iter().map(recorded_file).collect();
            restoration.persist(&files, &self.directories, &self.temporary_roots)
        })
    }
}
