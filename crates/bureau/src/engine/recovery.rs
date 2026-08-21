//! Discovery of unfinished durable run snapshots.

use std::path::Path;

use super::TerminalRecord;
use crate::runlog::{self, RunSnapshot, RunState, RunStatus};

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

fn replay_started(directory: &Path) -> std::io::Result<Option<RunState>> {
    let events = runlog::read_events(directory)?;
    if events.is_empty() {
        return Ok(None);
    }
    runlog::replay(events).map(Some).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "non-empty log has no run_started event",
        )
    })
}

fn valid_cost(usage: &crate::adapters::Usage) -> Option<f64> {
    usage
        .cost_usd
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
}

fn measured_cost(state: &RunState) -> f64 {
    let steps = state.steps.iter().filter_map(|step| step.usage.as_ref());
    let partial = state
        .groups
        .values()
        .filter(|group| group.usage.is_none())
        .flat_map(|group| group.members.values())
        .filter_map(|member| member.usage.as_ref());
    steps.chain(partial).filter_map(valid_cost).sum()
}

pub(super) fn unfinished(runs_dir: &Path) -> std::io::Result<Vec<RunSnapshot>> {
    let mut directories = directories(runs_dir)?;
    directories.sort();
    let mut snapshots = Vec::new();
    for directory in directories {
        let Some(state) = replay_started(&directory)? else {
            continue;
        };
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
        let Some(state) = replay_started(&directory)? else {
            continue;
        };
        if let (Some(snapshot), Some(finished)) = (state.snapshot, state.finished) {
            records.push(TerminalRecord { snapshot, finished });
        }
    }
    records.sort_by(|left, right| left.snapshot.run_id.cmp(&right.snapshot.run_id));
    Ok(records)
}

fn recovery_message(terminal: runlog::RunTerminal, message: &str) -> String {
    match terminal {
        runlog::RunTerminal::Done => message.to_owned(),
        runlog::RunTerminal::Abort => {
            format!("{message} (forge label unavailable during recovery)")
        }
        runlog::RunTerminal::Escalate => {
            format!("{message} (forge label and comment unavailable during recovery)")
        }
    }
}

const fn recovery_outcome(terminal: runlog::RunTerminal) -> crate::contract::StepOutcome {
    match terminal {
        runlog::RunTerminal::Abort => crate::contract::StepOutcome::Failure,
        runlog::RunTerminal::Done | runlog::RunTerminal::Escalate => {
            crate::contract::StepOutcome::Blocked
        }
    }
}

pub(super) fn finish(
    runs_dir: &Path,
    snapshot: &RunSnapshot,
    terminal: runlog::RunTerminal,
    message: &str,
) -> std::io::Result<()> {
    let _terminal = runlog::lock_terminal_append();
    let directory = runlog::run_dir(runs_dir, &snapshot.run_id);
    let state = runlog::replay_state(&directory)?;
    if state.finished.is_some() {
        return Ok(());
    }
    let mut log = runlog::RunLog::resume(&directory, &[])?;
    let message = recovery_message(terminal, message);
    log.append(
        runlog::EventKind::Output,
        runlog::output(None, "run", &message),
    )?;
    let outcome = recovery_outcome(terminal);
    let disposition = runlog::TerminalDisposition::for_outcome(outcome, state.pr.is_some());
    let finished = runlog::run_finished_full(
        Some(terminal),
        outcome,
        &message,
        measured_cost(&state),
        state.pr.as_ref(),
        disposition,
    );
    log.append(runlog::EventKind::RunFinished, finished)?;
    log.close()
}
