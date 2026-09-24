//! Advisory lock that serializes mutation of one bare mirror.
//!
//! `flock` locks belong to an open file description, so the lock
//! excludes other runs in this process and other `bureau` processes
//! alike. Waiters poll a non-blocking attempt, so the wait stays
//! bounded by the caller's timeout and never parks an executor thread.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

use super::Error;

/// How long a waiter sleeps between attempts on a busy lock.
const POLL: Duration = Duration::from_millis(25);

/// An exclusive hold on one mirror, released on drop.
pub struct MirrorLock {
    _lock: Flock<File>,
}

enum Attempt {
    Held(MirrorLock),
    Busy(File),
}

/// Opens (creating) the lock file and its directory.
fn open(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

async fn open_file(path: PathBuf) -> std::io::Result<File> {
    let opened = tokio::task::spawn_blocking(move || open(&path)).await;
    opened.map_err(std::io::Error::other)?
}

/// One non-blocking attempt; a busy lock hands the file back.
fn attempt(file: File) -> std::io::Result<Attempt> {
    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => Ok(Attempt::Held(MirrorLock { _lock: lock })),
        Err((file, Errno::EWOULDBLOCK | Errno::EINTR)) => Ok(Attempt::Busy(file)),
        Err((_, errno)) => Err(errno.into()),
    }
}

async fn poll(mut file: File) -> std::io::Result<MirrorLock> {
    loop {
        file = match attempt(file)? {
            Attempt::Held(lock) => return Ok(lock),
            Attempt::Busy(file) => file,
        };
        tokio::time::sleep(POLL).await;
    }
}

fn timed_out(path: &Path, wait: Duration) -> Error {
    let message = format!(
        "checkout cache lock `{}` stayed busy for {}s",
        path.display(),
        wait.as_secs()
    );
    std::io::Error::new(std::io::ErrorKind::TimedOut, message).into()
}

/// Takes the exclusive lock at `path`, waiting at most `wait`. One
/// attempt is always made, so a zero wait still takes a free lock.
pub(super) async fn acquire(path: PathBuf, wait: Duration) -> Result<MirrorLock, Error> {
    let file = open_file(path.clone()).await?;
    let held = tokio::time::timeout(wait, poll(file)).await;
    held.map_err(|_| timed_out(&path, wait))?
        .map_err(Error::from)
}
