//! `list`, `show`, and `cancel`: the read-side verbs over run dirs.
//!
//! The event log is the only source of truth; state is always replayed,
//! never trusted from the cache.

use std::path::Path;

use anyhow::Context as _;

use bureau::runlog::{
    self, BranchPushedData, CheckpointData, Event, EventKind, GroupFinishedData,
    GroupMemberCancelledData, GroupMemberFinishedData, GroupMemberStartedData, GroupStartedData,
    OutputData, PrCreatedData, RunFinishedData, RunStartedData, RunState, RunStatus,
    StepFinishedData, StepStartedData,
};

use super::outcome_name;

/// The marker file the engine checks between steps.
const CANCEL_FILE: &str = "CANCEL";

/// `list`: one line per run dir, sorted by run id.
pub fn list(runs: &Path) -> i32 {
    for line in list_lines(runs) {
        println!("{line}");
    }
    0
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

/// `running` or `finished(<outcome>)`.
fn status_text(status: &RunStatus) -> String {
    match status {
        RunStatus::Running => "running".to_owned(),
        RunStatus::Finished(outcome) => format!("finished({})", outcome_name(*outcome)),
    }
}

/// `show <run-id>`: the replayed state, then the last five events.
///
/// # Errors
/// Propagates an unreadable or headerless event log.
pub fn show(runs: &Path, run_id: &str) -> anyhow::Result<i32> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        eprintln!("no such run: `{run_id}`");
        return Ok(2);
    }
    let events = runlog::read_events(&dir).context("reading run events")?;
    let state = runlog::replay(events.clone()).context("run log has no run_started event")?;
    print_state(&state);
    print_tail(&events);
    Ok(0)
}

/// The replayed header: run id, assignment, status, and each step with
/// its outcome.
fn print_state(state: &RunState) {
    println!("run: {}", state.run_id);
    println!("assignment: {}", state.assignment);
    println!("status: {}", status_text(&state.status));
    println!("steps:");
    for step in &state.steps {
        let outcome = step.outcome.map_or("running", outcome_name);
        println!("  {}: {outcome}", step.step);
    }
}

/// The last five events, oldest first: `#<seq> <kind> <gist>`.
fn print_tail(events: &[Event]) {
    println!("events (last 5):");
    for event in events.iter().rev().take(5).rev() {
        println!("  #{} {} {}", event.seq, kind_name(event.kind), gist(event));
    }
}

/// The snake-case token for an event kind (its serde name).
const fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::RunStarted => "run_started",
        EventKind::StepStarted => "step_started",
        EventKind::Output => "output",
        EventKind::StepFinished => "step_finished",
        EventKind::GroupStarted => "group_started",
        EventKind::GroupMemberStarted => "group_member_started",
        EventKind::GroupMemberFinished => "group_member_finished",
        EventKind::GroupMemberCancelled => "group_member_cancelled",
        EventKind::GroupFinished => "group_finished",
        EventKind::Checkpoint => "checkpoint",
        EventKind::BranchPushed => "branch_pushed",
        EventKind::PrCreated => "pr_created",
        EventKind::RunFinished => "run_finished",
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
        EventKind::GroupStarted
        | EventKind::GroupMemberStarted
        | EventKind::GroupMemberFinished
        | EventKind::GroupMemberCancelled
        | EventKind::GroupFinished => group_gist(event),
        EventKind::Checkpoint | EventKind::BranchPushed | EventKind::PrCreated => {
            durable_gist(event)
        }
        EventKind::RunFinished => {
            payload::<RunFinishedData>(event).map_or_else(String::new, |d| run_gist(&d))
        }
    }
}

fn group_gist(event: &Event) -> String {
    match event.kind {
        EventKind::GroupStarted => {
            payload::<GroupStartedData>(event).map_or_else(String::new, |data| data.group)
        }
        EventKind::GroupMemberStarted => payload::<GroupMemberStartedData>(event)
            .map_or_else(String::new, |data| {
                format!("{}:{}", data.group, data.member)
            }),
        EventKind::GroupMemberFinished => payload::<GroupMemberFinishedData>(event)
            .map_or_else(String::new, |data| {
                format!("{}:{}", data.group, data.member)
            }),
        EventKind::GroupMemberCancelled => payload::<GroupMemberCancelledData>(event)
            .map_or_else(String::new, |data| {
                format!("{}:{}", data.group, data.member)
            }),
        EventKind::GroupFinished => {
            payload::<GroupFinishedData>(event).map_or_else(String::new, |data| data.group)
        }
        _ => String::new(),
    }
}

fn durable_gist(event: &Event) -> String {
    match event.kind {
        EventKind::Checkpoint => {
            payload::<CheckpointData>(event).map_or_else(String::new, |data| data.commit)
        }
        EventKind::BranchPushed => payload::<BranchPushedData>(event)
            .map_or_else(String::new, |data| {
                format!("{} {}", data.branch, data.commit)
            }),
        EventKind::PrCreated => {
            payload::<PrCreatedData>(event).map_or_else(String::new, |data| data.pr.url)
        }
        _ => String::new(),
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

/// `cancel <run-id>`: writes the CANCEL marker the engine checks between
/// steps.
///
/// # Errors
/// Propagates a failure to write the marker.
pub fn cancel(runs: &Path, run_id: &str) -> anyhow::Result<i32> {
    let dir = runlog::run_dir(runs, run_id);
    if !dir.is_dir() {
        eprintln!("no such run: `{run_id}`");
        return Ok(2);
    }
    if finished(&dir) {
        println!("run `{run_id}` is already finished");
        return Ok(1);
    }
    std::fs::write(dir.join(CANCEL_FILE), "cancelled\n").context("writing CANCEL marker")?;
    println!("run `{run_id}` marked for cancellation");
    Ok(0)
}

/// Whether the run's log already holds `run_finished`.
fn finished(dir: &Path) -> bool {
    runlog::read_events(dir)
        .map(|events| events.iter().any(|e| e.kind == EventKind::RunFinished))
        .unwrap_or(false)
}
