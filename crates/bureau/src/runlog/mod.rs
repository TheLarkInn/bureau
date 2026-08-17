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
mod gist;
mod group;
mod group_state;
mod snapshot;
mod state;

pub use bureau_plugin::PluginSource;
pub use event::{
    BranchPushedData, CheckpointData, Event, EventKind, OutputData, PrCreatedData, RunFinishedData,
    RunStartedData, StepFinishedData, StepStartedData, TerminalDisposition, branch_pushed,
    checkpoint, output, pr_created, run_finished, run_finished_full, run_started,
    run_started_for_item, run_started_snapshot, step_finished, step_finished_full, step_started,
};
pub use gist::{gist, kind_name, outcome_name, status_text};
pub use group::{
    GroupFinishedData, GroupMemberCancelledData, GroupMemberFinishedData, GroupMemberStartedData,
    GroupStartedData, group_finished, group_member_cancelled, group_member_finished,
    group_member_started, group_started,
};
pub use snapshot::{ConfigSource, RunSnapshot};
pub use state::{RunState, RunStatus, StepRecord};

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::adapters::Usage;
use crate::config::Completion;
use crate::contract::StepResult;
use crate::process::{Secret, scrub_json};

/// The append-only event log file name within a run directory.
pub const EVENTS_FILE: &str = "events.jsonl";

/// The derived state cache file name within a run directory.
pub const STATE_FILE: &str = "state.json";

/// Durable state for one concurrent group member.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupMemberRecord {
    /// Number of attempts that began.
    pub attempts: u32,
    /// Full result after the member finishes.
    pub result: Option<StepResult>,
    /// Adapter-owned usage after the member finishes.
    pub usage: Option<Usage>,
    /// Member encountered a non-routable control failure.
    #[serde(default)]
    pub halted: bool,
    /// Why the unfinished member was cancelled.
    pub cancellation_reason: Option<String>,
}

/// Durable state for one concurrent group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupRecord {
    /// Members keyed deterministically by step name.
    pub members: BTreeMap<String, GroupMemberRecord>,
    /// When unfinished members are cancelled.
    pub completion: Completion,
    /// Resolved positive member limit.
    pub max_concurrent: usize,
    /// Internal Git snapshot shared by every member.
    pub snapshot: String,
    /// Aggregate result after the group finishes.
    pub result: Option<StepResult>,
    /// Aggregate usage after the group finishes.
    pub usage: Option<Usage>,
    /// Aggregate must stop instead of following pipeline edges.
    #[serde(default)]
    pub halted: bool,
}

/// The directory one run writes into.
#[must_use]
pub fn run_dir(runs_dir: &Path, run_id: &str) -> PathBuf {
    runs_dir.join(run_id)
}

/// The process clock boundary: milliseconds since the Unix epoch. The
/// clock function is bound first so this helper stays the one place
/// naming the process clock.
fn now_millis() -> u64 {
    let now = SystemTime::now;
    now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Splits log text into non-empty lines, identifying a torn final line
/// (a daemon kill mid-append leaves one) without treating it as corrupt.
fn log_lines(text: &str) -> (Vec<&str>, Option<&str>) {
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let torn = lines
        .last()
        .is_some_and(|last| serde_json::from_str::<Event>(last).is_err())
        .then(|| lines.pop().unwrap_or_default());
    (lines, torn)
}

fn parse_events(lines: &[&str]) -> io::Result<Vec<Event>> {
    lines
        .iter()
        .map(|line| serde_json::from_str(line).map_err(io::Error::other))
        .collect()
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
    let (lines, torn) = log_lines(&text);
    if let Some(torn) = torn {
        let keep = torn.as_ptr() as usize - text.as_ptr() as usize;
        OpenOptions::new()
            .write(true)
            .open(&path)?
            .set_len(u64::try_from(keep).map_err(io::Error::other)?)?;
    }
    parse_events(&lines)
}

/// Read-only [`read_events`] for tools that must never mutate a run
/// directory: a torn final line is dropped from the result but the file
/// is left exactly as found.
///
/// # Errors
/// Propagates filesystem failures and rejects any unparseable line
/// before the last one.
pub fn read_events_tolerant(dir: &Path) -> io::Result<Vec<Event>> {
    let text = std::fs::read_to_string(dir.join(EVENTS_FILE))?;
    parse_events(&log_lines(&text).0)
}

/// An open run log. Appends are fsync'd and scrubbed on write.
pub struct RunLog {
    writer: BufWriter<File>,
    secrets: Vec<Secret>,
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
            writer: BufWriter::new(file),
            secrets: secrets.to_vec(),
            next_seq: 0,
            dir,
        })
    }

    /// Opens an existing log after repairing any torn final line.
    ///
    /// # Errors
    /// Propagates filesystem failures and rejects corrupt earlier events.
    pub fn resume(dir: &Path, secrets: &[Secret]) -> io::Result<Self> {
        let events = read_events(dir)?;
        let next_seq = events.last().map_or(0, |event| event.seq + 1);
        let file = OpenOptions::new()
            .append(true)
            .open(dir.join(EVENTS_FILE))?;
        Ok(Self {
            writer: BufWriter::new(file),
            secrets: secrets.to_vec(),
            next_seq,
            dir: dir.to_path_buf(),
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
    pub fn append(&mut self, kind: EventKind, mut data: serde_json::Value) -> io::Result<u64> {
        scrub_json(&mut data, &self.secrets);
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
        self.writer.get_ref().sync_all()?;
        self.next_seq += 1;
        Ok(event.seq)
    }

    /// Flushes the scrubber's retained tail and closes the log.
    ///
    /// # Errors
    /// Propagates write and flush failures.
    pub fn close(mut self) -> io::Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }
}

/// Rebuilds run state from the event log — the only source of truth.
///
/// Returns `None` when the log has no `run_started` event.
#[must_use]
pub fn replay(events: impl IntoIterator<Item = Event>) -> Option<RunState> {
    let mut iter = events.into_iter();
    let mut state = iter.by_ref().find_map(|e| RunState::from_event(&e))?;
    for event in iter {
        state.apply(&event);
    }
    Some(state)
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

/// Reads the derived state cache, when it exists and parses.
///
/// The engine writes the cache only when a run settles, so a running
/// run has none (or a torn one); callers then fall back to
/// [`read_events_tolerant`] + [`replay`].
#[must_use]
pub fn read_state_cache(dir: &Path) -> Option<RunState> {
    serde_json::from_slice(&std::fs::read(dir.join(STATE_FILE)).ok()?).ok()
}
