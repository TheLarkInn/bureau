//! Cross-process coordination for disposable local maintenance.

use std::fs::{File, OpenOptions};
use std::path::Path;

use nix::fcntl::{Flock, FlockArg};

/// Held local maintenance lock, released on drop.
pub struct Guard {
    _lock: Flock<File>,
}

/// Acquires a shared worker lock without waiting.
///
/// # Errors
/// Fails when maintenance is active or the lock file cannot be opened.
pub fn shared(home: &Path) -> Result<Guard, Error> {
    lock(home, FlockArg::LockSharedNonblock)
}

/// Acquires an exclusive maintenance lock without waiting.
///
/// # Errors
/// Fails when any worker is active or the lock file cannot be opened.
pub fn exclusive(home: &Path) -> Result<Guard, Error> {
    lock(home, FlockArg::LockExclusiveNonblock)
}

fn lock(home: &Path, argument: FlockArg) -> Result<Guard, Error> {
    std::fs::create_dir_all(home)?;
    let path = home.join("maintenance.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    Flock::lock(file, argument)
        .map(|lock| Guard { _lock: lock })
        .map_err(|(_, error)| Error::Busy(error.to_string()))
}

/// Maintenance coordination failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Another process holds an incompatible lock.
    #[error("local maintenance lock is busy: {0}")]
    Busy(String),
}
