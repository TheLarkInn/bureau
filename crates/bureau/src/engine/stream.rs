//! The streaming sink from a spawned step's output to the run log.
//!
//! Layer 0 multiplexes both child streams into the one
//! [`crate::process::SharedLog`] sink, so per-chunk stream attribution is
//! impossible here; events are labeled `combined` and say exactly that.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use super::log::Appender;
use crate::runlog::{self, EventKind};

/// The log handle shared between the machine and live step sinks.
pub(super) type Shared = Arc<Mutex<Appender>>;

/// Locks a run-log mutex, surviving poisoning: a panicking step writer
/// must not cascade into losing the run's events.
pub(super) fn lock(log: &Shared) -> MutexGuard<'_, Appender> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A [`Write`] sink appending one `output` event per chunk to the log.
pub(super) struct LogSink {
    step: String,
    log: Shared,
}

impl LogSink {
    /// A sink attributing chunks to `step`.
    pub(super) fn new(step: &str, log: &Shared) -> Self {
        Self {
            step: step.to_owned(),
            log: Arc::clone(log),
        }
    }
}

impl Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let data = String::from_utf8_lossy(buf);
        let event = runlog::output(Some(&self.step), "combined", &data);
        lock(&self.log).append(EventKind::Output, event)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
