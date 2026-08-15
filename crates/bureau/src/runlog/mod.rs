//! Layer 3: the run log (DESIGN.md section 7). One directory per run:
//!
//! ```text
//! runs/<run-id>/
//!   events.jsonl   # append-only, fsync'd, sequence-numbered, scrubbed
//!   state.json     # derived cache; reconstructible by replay
//!   artifacts/
//!   wt/            # the worktree
//! ```

mod event;
mod state;

pub use event::{
    Event, EventKind, OutputData, RunFinishedData, RunStartedData, StepFinishedData,
    StepStartedData, output, run_finished, run_started, run_started_for_item, step_finished,
    step_started,
};
pub use state::{RunState, RunStatus, StepRecord, replay};

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::process::{ScrubWriter, Secret};

/// The append-only event log file name within a run directory.
pub const EVENTS_FILE: &str = "events.jsonl";

/// The derived state cache file name within a run directory.
pub const STATE_FILE: &str = "state.json";

/// The directory one run writes into.
#[must_use]
pub fn run_dir(runs_dir: &Path, run_id: &str) -> PathBuf {
    runs_dir.join(run_id)
}

/// An open run log. Appends are fsync'd and scrubbed on write.
pub struct RunLog {
    writer: ScrubWriter<BufWriter<File>>,
    next_seq: u64,
    dir: PathBuf,
}

impl RunLog {
    /// Creates the run directory and its log. Fails if the log already
    /// exists — a run id is used exactly once.
    ///
    /// # Errors
    /// Propagates filesystem failures, including an existing log.
    pub fn create(runs_dir: &Path, run_id: &str, secrets: &[Secret]) -> io::Result<Self> {
        let dir = run_dir(runs_dir, run_id);
        std::fs::create_dir_all(dir.join("artifacts"))?;
        std::fs::create_dir_all(dir.join("wt"))?;
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(dir.join(EVENTS_FILE))?;
        Ok(Self {
            writer: ScrubWriter::new(BufWriter::new(file), secrets),
            next_seq: 0,
            dir,
        })
    }

    /// The directory this run writes into.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Appends one event; fsync'd before this returns.
    ///
    /// # Errors
    /// Propagates serialization, write, and sync failures.
    pub fn append(&mut self, kind: EventKind, data: serde_json::Value) -> io::Result<u64> {
        let event = Event {
            seq: self.next_seq,
            at_ms: now_millis(),
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
    pub fn close(self) -> io::Result<()> {
        self.writer.finish()?;
        Ok(())
    }
}

/// Reads every event in a run directory's log, in sequence order.
///
/// A daemon kill mid-append leaves the final line torn — the scrubber's
/// holdback tail is flushed only by [`RunLog::close`] — so for crash
/// recovery a torn LAST line is dropped, not an error, and is truncated
/// from the file (WAL-style repair on open; otherwise a resume's next
/// append would fuse onto the partial bytes and poison the log
/// mid-file). An unparseable line anywhere earlier remains an error.
///
/// # Errors
/// Propagates filesystem failures and rejects any unparseable line
/// before the last one.
pub fn read_events(dir: &Path) -> io::Result<Vec<Event>> {
    let path = dir.join(EVENTS_FILE);
    let text = std::fs::read_to_string(&path)?;
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines
        .last()
        .is_some_and(|last| serde_json::from_str::<Event>(last).is_err())
    {
        let torn = lines.pop().unwrap_or_default();
        let keep = torn.as_ptr() as usize - text.as_ptr() as usize;
        OpenOptions::new()
            .write(true)
            .open(&path)?
            .set_len(u64::try_from(keep).map_err(io::Error::other)?)?;
    }
    lines
        .iter()
        .map(|line| serde_json::from_str(line).map_err(io::Error::other))
        .collect()
}

/// Replays a run directory's log into its state.
///
/// # Errors
/// Propagates read failures; rejects a log with no `run_started` event.
pub fn replay_state(dir: &Path) -> io::Result<RunState> {
    replay(read_events(dir)?)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "log has no run_started event"))
}

/// Writes the derived state cache. It must always be reconstructible by
/// [`replay_state`]; never write anything to it that the log lacks.
///
/// # Errors
/// Propagates serialization and filesystem failures.
pub fn write_state_cache(dir: &Path, state: &RunState) -> io::Result<()> {
    std::fs::write(
        dir.join(STATE_FILE),
        serde_json::to_vec_pretty(state).map_err(io::Error::other)?,
    )
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
