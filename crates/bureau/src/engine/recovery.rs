//! Discovery of unfinished durable run snapshots.

use std::path::Path;

use super::TerminalRecord;
use crate::runlog::{self, RunSnapshot, RunStatus};

pub(super) fn unfinished(runs_dir: &Path) -> std::io::Result<Vec<RunSnapshot>> {
    let mut directories = directories(runs_dir)?;
    directories.sort();
    let mut snapshots = Vec::new();
    for directory in directories {
        let state = runlog::replay_state(&directory)?;
        if state.status == RunStatus::Running
            && let Some(snapshot) = state.snapshot
        {
            snapshots.push(snapshot);
        }
    }
    Ok(snapshots)
}

pub(super) fn finished(runs_dir: &Path) -> std::io::Result<Vec<TerminalRecord>> {
    let mut records = Vec::new();
    for directory in directories(runs_dir)? {
        let state = runlog::replay_state(&directory)?;
        if let (Some(snapshot), Some(finished)) = (state.snapshot, state.finished) {
            records.push(TerminalRecord { snapshot, finished });
        }
    }
    records.sort_by(|left, right| left.snapshot.run_id.cmp(&right.snapshot.run_id));
    Ok(records)
}

fn directories(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    Ok(std::fs::read_dir(root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(runlog::EVENTS_FILE).is_file())
        .collect())
}
