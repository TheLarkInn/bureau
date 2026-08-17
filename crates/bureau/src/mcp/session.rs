use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::contract::{StepRequest, StepResult};

use super::{BUREAU_STEP_REQUEST, BUREAU_STEP_RESULT, Paths, REQUEST_FILE, RESULT_FILE};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);

fn invalid_result(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn checked_request(request: &StepRequest) -> io::Result<Vec<u8>> {
    let bytes = request.to_json().map_err(io::Error::other)?;
    StepRequest::from_json(&bytes).map_err(invalid_result)?;
    Ok(bytes)
}

fn no_outside_directory() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "worktree contains every available temporary directory",
    )
}

fn outside_root(mut root: PathBuf, worktree: &Path) -> io::Result<PathBuf> {
    while root.starts_with(worktree) {
        root = root
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(no_outside_directory)?;
    }
    Ok(root)
}

fn temp_root(worktree: &Path) -> io::Result<PathBuf> {
    let worktree = std::fs::canonicalize(worktree)?;
    let root = std::fs::canonicalize(std::env::temp_dir())?;
    outside_root(root, &worktree)
}

/// The process clock boundary: the one place session naming reads the
/// wall clock, bound as a function pointer first.
fn clock_nanos() -> u128 {
    let now = SystemTime::now;
    now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn unique_name() -> String {
    let sequence = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let nanos = clock_nanos();
    format!("bureau-mcp-{}-{nanos}-{sequence}", std::process::id())
}

fn create_unique(root: &Path) -> io::Result<PathBuf> {
    for _ in 0..128 {
        let path = root.join(unique_name());
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a unique MCP session directory",
    ))
}

fn write_request(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn path_text(path: &Path) -> io::Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "MCP session path is not valid UTF-8",
        )
    })
}

fn environment(paths: &Paths) -> io::Result<BTreeMap<String, String>> {
    Ok(BTreeMap::from([
        (BUREAU_STEP_REQUEST.to_owned(), path_text(paths.request())?),
        (BUREAU_STEP_RESULT.to_owned(), path_text(paths.result())?),
    ]))
}

fn initialize(directory: &Path, bytes: &[u8]) -> io::Result<(Paths, BTreeMap<String, String>)> {
    let paths = Paths::new(directory.join(REQUEST_FILE), directory.join(RESULT_FILE));
    write_request(paths.request(), bytes)?;
    let env = environment(&paths)?;
    Ok((paths, env))
}

/// Temporary files and environment for one adapter invocation.
#[derive(Debug)]
pub struct Session {
    directory: PathBuf,
    paths: Paths,
    env: BTreeMap<String, String>,
}

impl Session {
    /// Creates a unique temporary session outside `request.worktree`.
    ///
    /// # Errors
    /// Returns an error when the request is invalid or files cannot be created.
    pub fn create(request: &StepRequest) -> io::Result<Self> {
        let bytes = checked_request(request)?;
        let directory = create_unique(&temp_root(&request.worktree)?)?;
        match initialize(&directory, &bytes) {
            Ok((paths, env)) => Ok(Self {
                directory,
                paths,
                env,
            }),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&directory);
                Err(error)
            }
        }
    }

    /// Temporary directory containing all files for this session.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.directory
    }

    /// Immutable request file.
    #[must_use]
    pub fn request_path(&self) -> &Path {
        self.paths.request()
    }

    /// Reserved result file. It does not exist before publication.
    #[must_use]
    pub fn result_path(&self) -> &Path {
        self.paths.result()
    }

    /// Environment entries adapters merge into the complete child environment.
    #[must_use]
    pub const fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Returns a clone of the server path pair.
    #[must_use]
    pub fn paths(&self) -> Paths {
        self.paths.clone()
    }

    /// Reads a published result, rejecting malformed or wrong-schema data.
    ///
    /// # Errors
    /// Returns an error for filesystem failures or invalid result JSON.
    pub fn published(&self) -> io::Result<Option<StepResult>> {
        match std::fs::read(self.result_path()) {
            Ok(bytes) => StepResult::from_json(&bytes)
                .map(Some)
                .map_err(invalid_result),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
