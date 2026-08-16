use std::path::Path;

use super::Guard;
use crate::{Error, restoration};

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
            files: loaded.files,
            directories: loaded.directories,
            temporary_roots: loaded.temporary_roots,
            restoration: Some(loaded.record),
            active: true,
        }
    }

    pub(super) fn persist(&self) -> Result<(), Error> {
        self.restoration.as_ref().map_or(Ok(()), |restoration| {
            restoration.persist(&self.files, &self.directories, &self.temporary_roots)
        })
    }
}
