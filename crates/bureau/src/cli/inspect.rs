//! `list`, `show`, and `cancel`: the read-side verbs over run dirs.
//!
//! The event log is the only source of truth; state is always replayed,
//! never trusted from the cache.

use crate::cli::out;
use std::path::Path;

use anyhow::Context as _;

use bureau::runlog::{
    self, Event, EventKind, RunState, gist, kind_name, outcome_name, status_text,
};

/// The marker file the engine checks between steps.
const CANCEL_FILE: &str = "CANCEL";

/// `<run_id>  <status>  <assignment>`; an unreadable or absent log
/// shows `unknown`.
fn list_line(name: &str, dir: &Path) -> String {
    match runlog::replay_state(dir) {
        Ok(state) => format!(
            "{name}  {}  {}",
            status_text(&state.status),
            state.assignment
        ),
        Err(_) => format!("{name}  unknown  unknown"),
    }
}

/// Every run dir's line; a missing `runs/` reads as empty.
fn list_lines(runs: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(runs) else {
        return Vec::new();
    };
    let mut lines: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            list_line(&name, &entry.path())
        })
        .collect();
    lines.sort();
    lines
}

/// `list`: one line per run dir, sorted by run id.
pub fn list(runs: &Path) -> i32 {
    for line in list_lines(runs) {
        out::line(format_args!("{line}"));
    }
    0
}

/// The replayed header: run id, assignment, status, and each step with
/// its outcome.
fn print_state(state: &RunState) {
    out::line(format_args!("run: {}", state.run_id));
    out::line(format_args!("assignment: {}", state.assignment));
    out::line(format_args!("status: {}", status_text(&state.status)));
    out::line(format_args!("steps:"));
    for step in &state.steps {
        let outcome = step.outcome.map_or("running", outcome_name);
        out::line(format_args!("  {}: {outcome}", step.step));
    }
}

/// The last five events, oldest first: `#<seq> <kind> <gist>`.
fn print_tail(events: &[Event]) {
    out::line(format_args!("events (last 5):"));
    for event in events.iter().rev().take(5).rev() {
        out::line(format_args!(
            "  #{} {} {}",
            event.seq,
            kind_name(event.kind),
            gist(event)
        ));
    }
}

/// `show <run-id>`: the replayed state, then the last five events.
///
/// # Errors
/// Propagates an unreadable or headerless event log.
pub fn show(runs: &Path, run_id: &str) -> anyhow::Result<i32> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        out::error(format_args!("no such run: `{run_id}`"));
        return Ok(2);
    }
    let events = runlog::read_events(&dir).context("reading run events")?;
    let state = runlog::replay(events.clone()).context("run log has no run_started event")?;
    print_state(&state);
    print_tail(&events);
    Ok(0)
}

/// Whether the run's log already holds `run_finished`.
fn finished(dir: &Path) -> bool {
    runlog::read_events(dir)
        .map(|events| events.iter().any(|e| e.kind == EventKind::RunFinished))
        .unwrap_or(false)
}

/// `cancel <run-id>`: writes the CANCEL marker the engine checks between
/// steps.
///
/// # Errors
/// Propagates a failure to write the marker.
pub fn cancel(runs: &Path, run_id: &str) -> anyhow::Result<i32> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        out::error(format_args!("no such run: `{run_id}`"));
        return Ok(2);
    }
    if finished(&dir) {
        out::line(format_args!("run `{run_id}` is already finished"));
        return Ok(1);
    }
    std::fs::write(dir.join(CANCEL_FILE), "cancelled\n").context("writing CANCEL marker")?;
    out::line(format_args!("run `{run_id}` marked for cancellation"));
    Ok(0)
}
