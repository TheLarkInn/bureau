use std::fs;
use std::path::Path;

use crate::doctor::state_db::{LeaseCounts, lease_counts};
use crate::runlog::{self, Event, RunState, RunStatus};

use super::LocalEffects;
use crate::doctor::{Observation, Status};

#[derive(Default)]
struct RunCounts {
    runs: usize,
    running: usize,
    stale_derived: usize,
    orphan_worktrees: usize,
}

impl LocalEffects {
    pub(super) fn inspect_recovery(&self) -> Result<Observation, String> {
        let runs = inspect_runs(self.layout.runs())?;
        let leases = lease_counts(self.layout.state_db())?;
        Ok(recovery_observation(&runs, &leases))
    }
}

pub(super) fn replay_run_read_only(directory: &Path) -> Result<RunState, String> {
    let path = directory.join(runlog::EVENTS_FILE);
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let lines: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let events = parse_events(&lines, &path)?;
    runlog::replay(events).ok_or_else(|| format!("{} has no run_started event", path.display()))
}

fn parse_events(lines: &[&str], path: &Path) -> Result<Vec<Event>, String> {
    let mut events = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        match serde_json::from_str(line) {
            Ok(event) => events.push(event),
            Err(_) if index + 1 == lines.len() => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    Ok(events)
}

fn inspect_runs(root: &Path) -> Result<RunCounts, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RunCounts::default());
        }
        Err(error) => return Err(format!("{}: {error}", root.display())),
    };
    let mut counts = RunCounts::default();
    for entry in entries {
        inspect_run_entry(entry, &mut counts)?;
    }
    Ok(counts)
}

fn inspect_run_entry(
    entry: std::io::Result<fs::DirEntry>,
    counts: &mut RunCounts,
) -> Result<(), String> {
    let entry = entry.map_err(|error| error.to_string())?;
    let directory = entry.path();
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{} is not a safe run directory",
            directory.display()
        ));
    }
    inspect_run(&directory, counts)
}

fn inspect_run(directory: &Path, counts: &mut RunCounts) -> Result<(), String> {
    if !directory.join(runlog::EVENTS_FILE).is_file() {
        counts.orphan_worktrees += usize::from(directory.join("wt").exists());
        return Ok(());
    }
    let state = replay_run_read_only(directory)?;
    counts.runs += 1;
    counts.running += usize::from(state.status == RunStatus::Running);
    counts.stale_derived += usize::from(!derived_matches(directory, &state));
    Ok(())
}

fn derived_matches(directory: &Path, expected: &RunState) -> bool {
    fs::read(directory.join(runlog::STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RunState>(&bytes).ok())
        .is_some_and(|state| state == *expected)
}

fn recovery_observation(runs: &RunCounts, leases: &LeaseCounts) -> Observation {
    let attention = runs.stale_derived + runs.orphan_worktrees + leases.expired;
    let message = format!(
        "{} runs replayed; {} running; {} active leases; {} repairable findings",
        runs.runs, runs.running, leases.active, attention
    );
    if attention == 0 {
        Observation::new(Status::Ok, "recovery_state_ok", message)
    } else {
        Observation::new(Status::Warning, "recovery_repairs_available", message)
    }
}
