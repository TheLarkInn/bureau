//! Fresh-admission exclusions derived only from authoritative pipeline events.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};

use super::{Event, EventKind, RunState, RunStatus, log_lines, parse_events, replay};
use crate::config::ForgeKind;

fn selected(state: &RunState) -> bool {
    state.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot
            .pipeline
            .steps
            .iter()
            .any(|step| step.copilot_factory.is_some())
    })
}

/// Whether this log still reserves its work item independently of any expiring lease.
#[must_use]
pub fn preserves_factory_work(state: &RunState) -> bool {
    let records = &state.copilot_factories.0;
    if !selected(state) && records.is_empty() && state.copilot_factory_error.is_none() {
        return false;
    }
    state.copilot_factory_error.is_some()
        || matches!(state.status, RunStatus::Running)
        || records.values().any(|record| !record.can_clean())
}

fn directory(entry: &DirEntry) -> io::Result<Option<PathBuf>> {
    let kind = entry.file_type()?;
    if kind.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "run directory is symlinked",
        ));
    }
    Ok(kind.is_dir().then(|| entry.path()))
}

fn directories(root: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        if let Some(path) = directory(&entry?)? {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn pending_file(error: &io::Error, active: bool) -> bool {
    active && error.kind() == io::ErrorKind::NotFound
}

fn contents(directory: &Path, active: bool) -> io::Result<Option<Vec<u8>>> {
    match fs::read(directory.join(super::EVENTS_FILE)) {
        Err(error) if pending_file(&error, active) => Ok(None),
        result => result.map(Some).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "reading preserved factory work in {}: {error}",
                    directory.display()
                ),
            )
        }),
    }
}

fn events(bytes: Vec<u8>, active: bool) -> io::Result<Option<Vec<Event>>> {
    // The first framed event is unpublished until its newline, even across a UTF-8 split.
    if active && !bytes.contains(&b'\n') {
        return Ok(None);
    }
    let text = String::from_utf8(bytes).map_err(io::Error::other)?;
    parse_events(&log_lines(&text).0).map(Some)
}

fn cloud_only(events: &[Event]) -> bool {
    !events.is_empty()
        && events
            .iter()
            .all(|event| event.kind == EventKind::GitHubCloud)
}

fn state(directory: &Path, active: bool) -> io::Result<Option<RunState>> {
    let Some(bytes) = contents(directory, active)? else {
        return Ok(None);
    };
    let Some(events) = events(bytes, active)? else {
        return Ok(None);
    };
    if cloud_only(&events) {
        return Ok(None);
    }
    replay(events).map(Some).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("run {} has no valid run_started event", directory.display()),
        )
    })
}

fn active(directory: &Path, runs: &BTreeSet<String>) -> bool {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| runs.contains(name))
}

fn matches_forge(kind: ForgeKind, name: &str) -> bool {
    matches!(
        (kind, name),
        (ForgeKind::Github, "github") | (ForgeKind::Ado, "ado")
    )
}

fn preserved_item(
    state: &RunState,
    assignment: &str,
    forge: &str,
) -> io::Result<Option<(String, String)>> {
    if !preserves_factory_work(state) {
        return Ok(None);
    }
    let snapshot = state.snapshot.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "preserved local factory has no run snapshot",
        )
    })?;
    if snapshot.run_id != state.run_id || snapshot.assignment.name != state.assignment {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preserved factory identity changed",
        ));
    }
    if snapshot.assignment.name != assignment
        || !matches_forge(snapshot.assignment.work.forge, forge)
    {
        return Ok(None);
    }
    Ok(Some((
        snapshot.item.external_id.clone(),
        state.run_id.clone(),
    )))
}

/// Reads preserved local-factory work without consulting or rewriting derived caches.
///
/// A fresh claim must repeat this read inside its `SQLite` admission transaction:
/// factory events and terminal/output appends use the same database ownership fence.
///
/// # Errors
/// Propagates unreadable, corrupt, or identity-inconsistent authoritative logs.
pub fn preserved_factory_work_with_active(
    runs_dir: &Path,
    assignment: &str,
    forge: &str,
    active_runs: &BTreeSet<String>,
) -> io::Result<BTreeMap<String, String>> {
    let mut work = BTreeMap::new();
    for directory in directories(runs_dir)? {
        if let Some(state) = state(&directory, active(&directory, active_runs))?
            && let Some((item, run)) = preserved_item(&state, assignment, forge)?
        {
            work.entry(item).or_insert(run);
        }
    }
    Ok(work)
}

/// Reads preserved factory work without a live-owner exemption for unpublished headers.
///
/// # Errors
/// Propagates unreadable, corrupt, or identity-inconsistent authoritative logs.
pub fn preserved_factory_work(
    runs_dir: &Path,
    assignment: &str,
    forge: &str,
) -> io::Result<BTreeMap<String, String>> {
    preserved_factory_work_with_active(runs_dir, assignment, forge, &BTreeSet::new())
}
