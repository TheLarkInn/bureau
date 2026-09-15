//! Fresh-admission exclusions derived only from authoritative pipeline events.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use super::{Event, EventKind, RunState, RunStatus, log_lines, parse_events};
use crate::config::ForgeKind;

mod decode;
mod source;

use decode::Prepared;
pub use source::FactorySource;

// Bounded catch-up accepts only fully validated output, which cannot change replayed state.
const FENCED_OUTPUT_BYTES: usize = 1024 * 1024;

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

fn events(bytes: &[u8]) -> io::Result<Vec<Event>> {
    let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
    let (lines, torn) = log_lines(text);
    if text.ends_with('\n') && torn.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "framed log event is invalid",
        ));
    }
    parse_events(&lines)
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

fn only_output(bytes: &[u8], remaining: &mut usize) -> bool {
    if bytes.len() > *remaining || !bytes.ends_with(b"\n") {
        return false;
    }
    *remaining -= bytes.len();
    events(bytes).is_ok_and(|events| events.iter().all(|event| event.kind == EventKind::Output))
}

/// Replay is prepared outside the fence and used only after exact authority revalidation.
pub struct FactoryHistory {
    source: FactorySource,
    states: Vec<Prepared>,
}

impl FactoryHistory {
    pub(crate) fn prepare(mut source: FactorySource, previous: Option<Self>) -> Self {
        let mut previous: BTreeMap<_, _> = previous
            .into_iter()
            .flat_map(|history| {
                history
                    .source
                    .runs
                    .into_iter()
                    .zip(history.states)
                    .map(|(source, state)| (source.path().to_owned(), (source, state)))
            })
            .collect();
        let states = source
            .runs
            .iter()
            .map(|source| {
                let prior = previous.remove(source.path());
                decode::prepare(source, prior)
            })
            .collect();
        source.release_tails();
        Self { source, states }
    }

    pub(crate) fn capture(&self, root: &Path) -> io::Result<FactorySource> {
        FactorySource::capture(root, Some(&self.source))
    }

    pub(crate) fn matches(&self, current: &FactorySource) -> bool {
        if !self.source.same_root(current) {
            return false;
        }
        let mut remaining = FENCED_OUTPUT_BYTES;
        self.source
            .runs
            .iter()
            .zip(&current.runs)
            .zip(&self.states)
            .all(|((before, after), state)| {
                state.bound()
                    && (before.same(after)
                        || (state.pipeline()
                            && before
                                .appended(after)
                                .and_then(|appended| appended.bytes.as_deref())
                                .is_some_and(|bytes| only_output(bytes, &mut remaining))))
            })
    }

    pub(crate) fn work(
        self,
        assignment: &str,
        forge: &str,
        active_runs: &BTreeSet<String>,
    ) -> io::Result<BTreeMap<String, String>> {
        let mut work = BTreeMap::new();
        for (source, state) in self.source.runs.into_iter().zip(self.states) {
            if source.unpublished() && active(source.path(), active_runs) {
                continue;
            }
            if let Some(state) = state.state()?
                && let Some((item, run)) = preserved_item(&state, assignment, forge)?
            {
                work.entry(item).or_insert(run);
            }
        }
        Ok(work)
    }
}

/// Reads preserved local-factory work without consulting or rewriting derived caches.
///
/// Scheduling instead prepares replay outside its `SQLite` transaction and revalidates
/// these exact sources inside the same fence used by factory/terminal/output appends.
///
/// # Errors
/// Propagates unreadable, corrupt, or identity-inconsistent authoritative logs.
pub fn preserved_factory_work_with_active(
    runs_dir: &Path,
    assignment: &str,
    forge: &str,
    active_runs: &BTreeSet<String>,
) -> io::Result<BTreeMap<String, String>> {
    FactoryHistory::prepare(FactorySource::read(runs_dir)?, None).work(
        assignment,
        forge,
        active_runs,
    )
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
