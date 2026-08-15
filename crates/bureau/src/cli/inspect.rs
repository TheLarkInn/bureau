//! `list`, `show`, and `cancel`: the read-side verbs over run dirs.
//!
//! The event log is the only source of truth; state is always replayed,
//! never trusted from the cache.

use std::path::Path;

use anyhow::Context as _;

use bureau::runlog::{
    self, Event, EventKind, OutputData, RunFinishedData, RunStartedData, RunState, RunStatus,
    StepFinishedData, StepStartedData,
};

use super::{Line, outcome_name};

/// The marker file the engine checks between steps.
const CANCEL_FILE: &str = "CANCEL";

/// `running` or `finished(<outcome>)`.
fn status_text(status: &RunStatus) -> String {
    match status {
        RunStatus::Running => "running".to_owned(),
        RunStatus::Finished(outcome) => format!("finished({})", outcome_name(*outcome)),
    }
}

/// The snake-case token for an event kind (its serde name).
const fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::RunStarted => "run_started",
        EventKind::StepStarted => "step_started",
        EventKind::Output => "output",
        EventKind::StepFinished => "step_finished",
        EventKind::RunFinished => "run_finished",
    }
}

/// Deserializes an event payload; a mismatch displays as an empty gist.
fn payload<T: serde::de::DeserializeOwned>(event: &Event) -> Option<T> {
    serde_json::from_value(event.data.clone()).ok()
}

/// `run_started`: the assignment and item.
fn started_gist(data: RunStartedData) -> String {
    let item = data.item.unwrap_or_else(|| "none".to_owned());
    format!("assignment={} item={item}", data.assignment)
}

/// `step_finished`: the step and its outcome.
fn step_gist(data: &StepFinishedData) -> String {
    format!("{} -> {}", data.step, outcome_name(data.outcome))
}

/// `output`: the stream and a trimmed chunk.
fn output_gist(data: &OutputData) -> String {
    format!("{}: {}", data.stream, data.data.trim_end())
}

/// `run_finished`: the outcome.
fn run_gist(data: &RunFinishedData) -> String {
    outcome_name(data.outcome).to_owned()
}

/// Whether the run's log already holds `run_finished`.
fn finished(dir: &Path) -> bool {
    runlog::read_events(dir)
        .map(|events| events.iter().any(|e| e.kind == EventKind::RunFinished))
        .unwrap_or(false)
}

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

/// A one-line, message-ish summary of an event's payload.
fn gist(event: &Event) -> String {
    match event.kind {
        EventKind::RunStarted => {
            payload::<RunStartedData>(event).map_or_else(String::new, started_gist)
        }
        EventKind::StepStarted => {
            payload::<StepStartedData>(event).map_or_else(String::new, |d| d.step)
        }
        EventKind::StepFinished => {
            payload::<StepFinishedData>(event).map_or_else(String::new, |d| step_gist(&d))
        }
        EventKind::Output => {
            payload::<OutputData>(event).map_or_else(String::new, |d| output_gist(&d))
        }
        EventKind::RunFinished => {
            payload::<RunFinishedData>(event).map_or_else(String::new, |d| run_gist(&d))
        }
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

/// The replayed header lines: run id, assignment, status, and each
/// step with its outcome.
fn push_state(state: &RunState, lines: &mut Vec<Line>) {
    lines.push(Line::Out(format!("run: {}", state.run_id)));
    lines.push(Line::Out(format!("assignment: {}", state.assignment)));
    lines.push(Line::Out(format!("status: {}", status_text(&state.status))));
    lines.push(Line::Out("steps:".to_owned()));
    for step in &state.steps {
        let outcome = step.outcome.map_or("running", outcome_name);
        lines.push(Line::Out(format!("  {}: {outcome}", step.step)));
    }
}

/// The last five events, oldest first: `#<seq> <kind> <gist>`.
fn push_tail(events: &[Event], lines: &mut Vec<Line>) {
    lines.push(Line::Out("events (last 5):".to_owned()));
    for event in events.iter().rev().take(5).rev() {
        let gist = gist(event);
        lines.push(Line::Out(format!(
            "  #{} {} {}",
            event.seq,
            kind_name(event.kind),
            gist
        )));
    }
}

/// `list`: one line per run dir, sorted by run id.
pub fn list(runs: &Path, lines: &mut Vec<Line>) -> i32 {
    lines.extend(list_lines(runs).into_iter().map(Line::Out));
    0
}

/// `show <run-id>`: the replayed state, then the last five events.
///
/// # Errors
/// Propagates an unreadable or headerless event log.
pub fn show(runs: &Path, run_id: &str, lines: &mut Vec<Line>) -> anyhow::Result<i32> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        lines.push(Line::Err(format!("no such run: `{run_id}`")));
        return Ok(2);
    }
    let events = runlog::read_events(&dir).context("reading run events")?;
    let state = runlog::replay(events.clone()).context("run log has no run_started event")?;
    push_state(&state, lines);
    push_tail(&events, lines);
    Ok(0)
}

/// `cancel <run-id>`: writes the CANCEL marker the engine checks between
/// steps.
///
/// # Errors
/// Propagates a failure to write the marker.
pub fn cancel(runs: &Path, run_id: &str, lines: &mut Vec<Line>) -> anyhow::Result<i32> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        lines.push(Line::Err(format!("no such run: `{run_id}`")));
        return Ok(2);
    }
    if finished(&dir) {
        lines.push(Line::Out(format!("run `{run_id}` is already finished")));
        return Ok(1);
    }
    std::fs::write(dir.join(CANCEL_FILE), "cancelled\n").context("writing CANCEL marker")?;
    lines.push(Line::Out(format!("run `{run_id}` marked for cancellation")));
    Ok(0)
}
