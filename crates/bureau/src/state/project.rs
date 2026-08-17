//! Idempotent projection from terminal event log into scheduler state.

use std::path::Path;

use crate::runlog::{self, RunFinishedData, RunSnapshot, TerminalDisposition};

use super::{Disposition, Error, Store};

/// One terminal log and the immutable run inputs it projects into state.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalRecord {
    /// Immutable run inputs.
    pub snapshot: RunSnapshot,
    /// Complete terminal event.
    pub finished: RunFinishedData,
}

const fn disposition(value: Option<TerminalDisposition>) -> Option<Disposition> {
    match value {
        Some(TerminalDisposition::Proposed) => Some(Disposition::Proposed),
        Some(TerminalDisposition::NoChange) => Some(Disposition::NoChange),
        None => None,
    }
}

/// Idempotently projects one complete terminal record.
///
/// # Errors
/// Propagates durable-state failures.
pub fn project_terminal(store: &Store, record: &TerminalRecord) -> Result<(), Error> {
    let snapshot = &record.snapshot;
    store.record_run(
        &snapshot.run_id,
        &snapshot.assignment.name,
        record.finished.cost_usd,
    )?;
    if let Some(disposition) = disposition(record.finished.disposition) {
        store.mark_seen(&snapshot.item.content_hash(), disposition)?;
    }
    store.release_terminal(
        &snapshot.assignment.name,
        &snapshot.item.external_id,
        &snapshot.run_id,
    )
}

/// Projects one run's terminal event when it exists.
///
/// # Errors
/// Propagates durable-state failures.
pub fn project_run(store: &Store, runs_dir: &Path, run_id: &str) -> Result<bool, Error> {
    let directory = runlog::run_dir(runs_dir, run_id);
    if !directory.is_dir() {
        return Ok(false);
    }
    let state = runlog::replay_state(&directory)?;
    let (Some(snapshot), Some(finished)) = (state.snapshot, state.finished) else {
        return Ok(false);
    };
    project_terminal(store, &TerminalRecord { snapshot, finished })?;
    Ok(true)
}
