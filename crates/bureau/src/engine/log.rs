//! The run-log append handle the machine writes through.
//!
//! [`runlog::RunLog`] creates a run's log but cannot open an existing
//! one, which resume needs. This handle mirrors `RunLog`'s on-disk
//! behavior exactly — the same [`Event`] wire form, scrub-on-write, and
//! an fsync per append — for both the fresh and resumed cases, so the
//! machine has one append path.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::process::{ScrubWriter, Secret};
use crate::runlog::{self, Event, EventKind};

/// An open run log. Appends are fsync'd and scrubbed on write, exactly
/// as [`runlog::RunLog`] appends them.
pub(super) struct Appender {
    writer: ScrubWriter<BufWriter<File>>,
    next_seq: u64,
    dir: PathBuf,
    /// Wall clock (millis since the Unix epoch) stamping each event.
    clock: fn() -> u64,
}

impl Appender {
    /// Creates the run directory and its log. Fails if the log already
    /// exists — a run id is used exactly once.
    ///
    /// # Errors
    /// Propagates filesystem failures, including an existing log.
    pub(super) fn create(
        runs_dir: &Path,
        run_id: &str,
        secrets: &[Secret],
        clock: fn() -> u64,
    ) -> io::Result<Self> {
        let dir = runlog::run_dir(runs_dir, run_id);
        std::fs::create_dir_all(dir.join("artifacts"))?;
        std::fs::create_dir_all(dir.join("wt"))?;
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(dir.join(runlog::EVENTS_FILE))?;
        Ok(Self {
            writer: ScrubWriter::new(BufWriter::new(file), secrets),
            next_seq: 0,
            dir,
            clock,
        })
    }

    /// Opens an existing run's log for resume, continuing the sequence
    /// after the last recorded event.
    ///
    /// # Errors
    /// Propagates filesystem failures and rejects an unparseable log.
    pub(super) fn resume(dir: &Path, secrets: &[Secret], clock: fn() -> u64) -> io::Result<Self> {
        let events = runlog::read_events(dir)?;
        let next_seq = events.last().map_or(0, |event| event.seq + 1);
        let file = OpenOptions::new()
            .append(true)
            .open(dir.join(runlog::EVENTS_FILE))?;
        Ok(Self {
            writer: ScrubWriter::new(BufWriter::new(file), secrets),
            next_seq,
            dir: dir.to_path_buf(),
            clock,
        })
    }

    /// The directory this run writes into.
    pub(super) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Appends one event; fsync'd before this returns.
    ///
    /// # Errors
    /// Propagates serialization, write, and sync failures.
    pub(super) fn append(&mut self, kind: EventKind, data: serde_json::Value) -> io::Result<u64> {
        let event = Event {
            seq: self.next_seq,
            at_ms: (self.clock)(),
            kind,
            data,
        };
        let mut line = serde_json::to_vec(&event).map_err(io::Error::other)?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        self.writer.flush()?;
        self.writer.get_ref().get_ref().sync_all()?;
        self.next_seq += 1;
        Ok(event.seq)
    }

    /// Flushes the scrubber's retained tail and closes the log.
    ///
    /// # Errors
    /// Propagates write and flush failures.
    pub(super) fn close(self) -> io::Result<()> {
        self.writer.finish()?;
        Ok(())
    }
}
