//! The streaming sink from a spawned step's output to the run log.
//!
//! Layer 0 multiplexes both child streams into the one
//! [`crate::process::SharedLog`] sink, so per-chunk stream attribution is
//! impossible here; events are labeled `combined` and say exactly that.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::runlog::{self, EventKind, RunLog};
use crate::state;

/// The log handle shared between the machine and live step sinks.
pub(super) type Shared = Arc<Mutex<RunLog>>;

/// Locks a run-log mutex, surviving poisoning: a panicking step writer
/// must not cascade into losing the run's events.
pub(super) fn lock(log: &Shared) -> MutexGuard<'_, RunLog> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write_error(error: state::Error) -> io::Error {
    match error {
        state::Error::Io(error) => error,
        error @ state::Error::LeaseLost(_) => {
            io::Error::new(io::ErrorKind::PermissionDenied, error)
        }
        error => io::Error::other(error),
    }
}

fn with_log<T>(
    log: &Shared,
    owner: Option<&state::LeaseOwner>,
    operation: impl FnOnce(&mut RunLog) -> io::Result<T>,
) -> io::Result<T> {
    let append = || operation(&mut lock(log));
    match owner {
        Some(owner) => owner.with_ownership(append).map_err(write_error),
        None => append(),
    }
}

pub(super) fn append(
    log: &Shared,
    owner: Option<&state::LeaseOwner>,
    kind: EventKind,
    data: serde_json::Value,
) -> io::Result<()> {
    with_log(log, owner, |log| log.append(kind, data).map(|_| ()))
}

/// A [`Write`] sink appending one `output` event per chunk to the log.
pub(super) struct LogSink {
    step: String,
    log: Shared,
    owner: Option<state::LeaseOwner>,
}

impl LogSink {
    /// A sink attributing chunks to `step`.
    pub(super) fn new(step: &str, log: &Shared, owner: Option<state::LeaseOwner>) -> Self {
        Self {
            step: step.to_owned(),
            log: Arc::clone(log),
            owner,
        }
    }
}

impl Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let data = String::from_utf8_lossy(buf);
        let event = runlog::output(Some(&self.step), "combined", &data);
        append(&self.log, self.owner.as_ref(), EventKind::Output, event)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
