mod path_ops;
mod storage;

use std::fs;
use std::path::{Path, PathBuf};

use super::{Error, package};
use path_ops::{
    DIRECTORY, absolute, absolute_paths, activation_id, activation_root, canonical_directory,
    canonical_optional, contained_worktree, record_path, reject_existing, relative,
};
use storage::{FileState, Manifest, OriginalState};

/// Pre-activation file contents and mode.
#[derive(Debug)]
pub struct RecordedOriginal {
    /// Exact pre-activation bytes.
    pub bytes: Vec<u8>,
    /// Unix permission mode bits.
    pub mode: u32,
}

/// One activation file's exact state at an absolute worktree path.
#[derive(Debug)]
pub struct RecordedFile {
    /// Absolute path inside the activation worktree.
    pub path: PathBuf,
    /// Pre-activation bytes and mode, when the file existed.
    pub original: Option<RecordedOriginal>,
    /// Bytes injected for the activation.
    pub injected: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub activation_id: String,
    pub plugin: String,
    pub recorded_version: String,
    pub installed_version: String,
}

#[derive(Debug, Default)]
pub struct Record {
    path: PathBuf,
    run_dir: PathBuf,
    worktree: PathBuf,
    plugin: String,
    version: String,
    source: Option<PathBuf>,
}

impl Record {
    pub fn create(
        run_dir: &Path,
        worktree: &Path,
        plugin: &str,
        version: &str,
        source: Option<&Path>,
    ) -> Result<Self, Error> {
        let (run_dir, worktree) = contained_worktree(run_dir, worktree)?;
        let relative = relative(&run_dir, &worktree)?;
        let activation_id = activation_id(&relative);
        let root = activation_root(&run_dir)?;
        let path = root.join(format!("{activation_id}.json"));
        reject_existing(&path)?;
        let record = Self {
            path,
            run_dir,
            worktree,
            plugin: plugin.to_owned(),
            version: version.to_owned(),
            source: canonical_optional(source)?,
        };
        record.persist(&[], &[], &[])?;
        Ok(record)
    }
    pub fn persist(
        &self,
        files: &[RecordedFile],
        directories: &[PathBuf],
        temporary_roots: &[PathBuf],
    ) -> Result<(), Error> {
        let manifest = Manifest {
            plugin: self.plugin.clone(),
            version: self.version.clone(),
            worktree: relative(&self.run_dir, &self.worktree)?,
            source: self.source.clone(),
            files: files
                .iter()
                .map(|file| self.saved_file(file))
                .collect::<Result<_, _>>()?,
            directories: self.relative_paths(directories)?,
            temporary_roots: self.relative_paths(temporary_roots)?,
        };
        storage::write(&self.path, &manifest)
    }

    pub fn remove(&self) -> Result<(), Error> {
        fs::remove_file(&self.path)
            .map_err(|error| Error::io("remove restoration", &self.path, error))?;
        if let Some(parent) = self.path.parent() {
            let _removed = fs::remove_dir(parent);
        }
        Ok(())
    }

    fn saved_file(&self, file: &RecordedFile) -> Result<FileState, Error> {
        Ok(FileState {
            path: relative(&self.worktree, &file.path)?,
            original: file.original.as_ref().map(|original| OriginalState {
                bytes: original.bytes.clone(),
                mode: original.mode,
            }),
            injected: file.injected.clone(),
        })
    }

    fn relative_paths(&self, paths: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
        paths
            .iter()
            .map(|path| relative(&self.worktree, path))
            .collect()
    }

    fn loaded(path: PathBuf, run_dir: PathBuf, worktree: PathBuf, manifest: &Manifest) -> Self {
        Self {
            path,
            run_dir,
            worktree,
            plugin: manifest.plugin.clone(),
            version: manifest.version.clone(),
            source: manifest.source.clone(),
        }
    }
}

pub struct Loaded {
    pub files: Vec<RecordedFile>,
    pub directories: Vec<PathBuf>,
    pub temporary_roots: Vec<PathBuf>,
    pub record: Record,
    pub info: Info,
}

fn required_version(manifest: &Manifest) -> Result<String, Error> {
    if manifest.version == "pinned" && manifest.source.is_none() {
        return Ok("pinned".to_owned());
    }
    let source = manifest
        .source
        .as_deref()
        .ok_or_else(|| Error::invalid(Path::new(DIRECTORY), "plugin source is unavailable"))?;
    package::version(source)
}

fn observed_version(manifest: &Manifest) -> String {
    required_version(manifest).unwrap_or_else(|_| "<unavailable>".to_owned())
}

fn info(path: &Path) -> Result<Info, Error> {
    let manifest = storage::read(path)?;
    let installed_version = observed_version(&manifest);
    let activation_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::invalid(path, "restoration record has no identity"))?;
    storage::validate_id(activation_id)?;
    Ok(Info {
        activation_id: activation_id.to_owned(),
        plugin: manifest.plugin,
        installed_version,
        recorded_version: manifest.version,
    })
}

pub fn infos(run_dir: &Path) -> Result<Vec<Info>, Error> {
    let root = run_dir.join(DIRECTORY);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io("read restorations", &root, error)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| Error::io("read restoration", &root, error))?
            .path();
        if storage::is_finalized(&path) {
            found.push(info(&path)?);
        }
    }
    found.sort_by(|left, right| left.activation_id.cmp(&right.activation_id));
    Ok(found)
}

struct LoadedPaths {
    files: Vec<RecordedFile>,
    directories: Vec<PathBuf>,
    temporary_roots: Vec<PathBuf>,
}

fn recorded_file(worktree: &Path, state: FileState) -> Result<RecordedFile, Error> {
    Ok(RecordedFile {
        path: absolute(worktree, &state.path)?,
        original: state.original.map(|original| RecordedOriginal {
            bytes: original.bytes,
            mode: original.mode,
        }),
        injected: state.injected,
    })
}

fn loaded_paths(
    worktree: &Path,
    files: Vec<FileState>,
    directories: &[PathBuf],
    temporary_roots: &[PathBuf],
) -> Result<LoadedPaths, Error> {
    let files = files
        .into_iter()
        .map(|file| recorded_file(worktree, file))
        .collect::<Result<_, _>>()?;
    Ok(LoadedPaths {
        files,
        directories: absolute_paths(worktree, directories)?,
        temporary_roots: absolute_paths(worktree, temporary_roots)?,
    })
}

fn loaded_info(
    activation_id: &str,
    plugin: String,
    recorded_version: String,
    installed_version: String,
) -> Info {
    Info {
        activation_id: activation_id.to_owned(),
        plugin,
        recorded_version,
        installed_version,
    }
}

pub fn load(run_dir: &Path, activation_id: &str) -> Result<Loaded, Error> {
    storage::validate_id(activation_id)?;
    let run_dir = canonical_directory(run_dir)?;
    let path = record_path(&run_dir, activation_id);
    let manifest = storage::read(&path)?;
    let worktree = absolute(&run_dir, &manifest.worktree)?;
    let installed_version = required_version(&manifest)?;
    let record = Record::loaded(path, run_dir, worktree.clone(), &manifest);
    let paths = loaded_paths(
        &worktree,
        manifest.files,
        &manifest.directories,
        &manifest.temporary_roots,
    )?;
    Ok(Loaded {
        files: paths.files,
        directories: paths.directories,
        temporary_roots: paths.temporary_roots,
        info: loaded_info(
            activation_id,
            manifest.plugin,
            manifest.version,
            installed_version,
        ),
        record,
    })
}
