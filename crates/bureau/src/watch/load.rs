//! Loads one frame from disk. Every input is optional and every read
//! is tolerant: a missing home, an absent state.db, a run directory
//! being written mid-read, or a directory disappearing between listing
//! and reading degrades to a note, never a panic.

use std::path::{Path, PathBuf};

use super::model::{BudgetRow, Frame, Header, RunRow};
use crate::config::{ActivatedConfig, Assignment};
use crate::runlog::{
    self, Event, RunState, RunStatus, gist, kind_name, read_events_tolerant, read_state_cache,
    replay,
};
use crate::state::{Budget, Store};

/// The filesystem roots watch reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    /// Directory holding run directories.
    pub runs: PathBuf,
    /// Durable state database path.
    pub state: PathBuf,
    /// Committed config cache directory (holds `active.json`).
    pub config_cache: PathBuf,
}

/// The adopted config revision, when the cache exists and parses.
fn read_active(roots: &Roots, notes: &mut Vec<String>) -> Option<ActivatedConfig> {
    let path = roots.config_cache.join("active.json");
    let bytes = std::fs::read(&path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(active) => Some(active),
        Err(error) => {
            notes.push(format!("{}: {error}", path.display()));
            None
        }
    }
}

/// The read-only store, when state.db exists and opens.
fn open_store(roots: &Roots, notes: &mut Vec<String>) -> Option<Store> {
    if !roots.state.exists() {
        return None;
    }
    match Store::open_read_only(&roots.state) {
        Ok(store) => Some(store),
        Err(error) => {
            notes.push(format!("state database unreadable: {error}"));
            None
        }
    }
}

/// The item a run targets; `-` when the run predates item tracking.
fn item(state: &RunState) -> String {
    state.snapshot.as_ref().map_or_else(
        || "-".to_owned(),
        |snapshot| snapshot.item.external_id.clone(),
    )
}

/// Latest step and its state.
fn step_text(state: &RunState) -> String {
    let Some(step) = state.steps.last() else {
        return match state.status {
            RunStatus::Running => "starting".to_owned(),
            RunStatus::Finished(_) => "-".to_owned(),
        };
    };
    step.outcome.map_or_else(
        || format!("{}: running", step.step),
        |outcome| format!("{}: {}", step.step, runlog::outcome_name(outcome)),
    )
}

/// Cost so far: the terminal total when finished, else the sum of
/// per-step measured costs when any step reported one.
fn cost(state: &RunState) -> Option<f64> {
    if let Some(finished) = &state.finished {
        return Some(finished.cost_usd);
    }
    let mut total = None;
    for step in &state.steps {
        if let Some(cost) = step.usage.as_ref().and_then(|usage| usage.cost_usd) {
            total = Some(total.unwrap_or(0.0) + cost);
        }
    }
    total
}

/// The state cache when it exists, parses, and says the run settled.
/// (A cache written at teardown is terminal; a live or resumed run
/// always falls through to replay.)
fn settled_cache(dir: &Path) -> Option<RunState> {
    let state = read_state_cache(dir)?;
    matches!(state.status, RunStatus::Finished(_)).then_some(state)
}

fn replayed(dir: &Path) -> Option<RunState> {
    replay(read_events_tolerant(dir).ok()?)
}

/// One run's state from the cache or a replay of its log.
fn run_state(dir: &Path) -> Option<RunState> {
    settled_cache(dir).or_else(|| replayed(dir))
}

/// One run directory's row. An unreadable run is skipped by the caller.
fn load_run(dir: &Path, now_ms: u64) -> Option<RunRow> {
    let state = run_state(dir)?;
    Some(RunRow {
        run_id: state.run_id.clone(),
        assignment: state.assignment.clone(),
        item: item(&state),
        step: step_text(&state),
        cost_usd: cost(&state),
        started_at_ms: state.started_at_ms,
        age_ms: now_ms.saturating_sub(state.started_at_ms),
        status: state.status,
    })
}

/// Running first, then newest first.
fn sort_rows(rows: &mut [RunRow]) {
    rows.sort_by(|a, b| {
        b.is_running()
            .cmp(&a.is_running())
            .then(b.started_at_ms.cmp(&a.started_at_ms))
            .then_with(|| a.run_id.cmp(&b.run_id))
    });
}

/// Runs-dir entries, when the directory is readable.
fn run_entries(runs: &Path, notes: &mut Vec<String>) -> Vec<PathBuf> {
    match std::fs::read_dir(runs) {
        Ok(entries) => entries.filter_map(Result::ok).map(|e| e.path()).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            notes.push(format!("{}: {error}", runs.display()));
            Vec::new()
        }
    }
}

/// One entry's row, or a skipped-run tally when a real run directory
/// (not a stray file) could not be read.
fn push_row(rows: &mut Vec<RunRow>, skipped: &mut usize, dir: &Path, now_ms: u64) {
    if let Some(row) = load_run(dir, now_ms) {
        rows.push(row);
    } else if dir.is_dir() {
        *skipped += 1;
    }
}

/// Every run row. Unreadable runs are counted into a note, not listed
/// as fake rows.
fn load_runs(runs: &Path, now_ms: u64, notes: &mut Vec<String>) -> Vec<RunRow> {
    let mut rows = Vec::new();
    let mut skipped = 0_usize;
    for dir in run_entries(runs, notes) {
        push_row(&mut rows, &mut skipped, &dir, now_ms);
    }
    if skipped > 0 {
        notes.push(format!("{skipped} run(s) unreadable"));
    }
    sort_rows(&mut rows);
    rows
}

fn budget_row(assignment: &Assignment, budget: Budget, headroom: Option<usize>) -> BudgetRow {
    BudgetRow {
        assignment: assignment.name.clone(),
        spent_usd: budget.spent_today_usd,
        max_cost_usd: assignment.limits.max_cost_per_day_usd,
        runs_hour: budget.runs_this_hour,
        max_runs_hour: assignment.limits.max_runs_per_hour,
        headroom,
    }
}

/// One budget row per configured assignment. Needs both the adopted
/// config (assignment names and limits) and the store (counters).
fn load_budgets(
    active: Option<&ActivatedConfig>,
    store: Option<&Store>,
    notes: &mut Vec<String>,
) -> Vec<BudgetRow> {
    let (Some(active), Some(store)) = (active, store) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for assignment in active.config.assignments.values() {
        match store.budget(&assignment.name) {
            Ok(budget) => {
                let headroom = store.headroom(&assignment.name, &assignment.limits, 0).ok();
                rows.push(budget_row(assignment, budget, headroom));
            }
            Err(error) => {
                notes.push(format!(
                    "budget for `{}` unreadable: {error}",
                    assignment.name
                ));
            }
        }
    }
    rows
}

/// The selected run's most recent events as `#<seq> <kind> <gist>`.
fn tail_lines(events: &[Event], count: usize) -> Vec<String> {
    events
        .iter()
        .rev()
        .take(count)
        .rev()
        .map(|event| format!("#{} {} {}", event.seq, kind_name(event.kind), gist(event)))
        .collect()
}

/// The detail pane's lines for `run_id`.
fn load_detail(
    runs: &Path,
    run_id: Option<&str>,
    lines: usize,
    notes: &mut Vec<String>,
) -> (Option<String>, Vec<String>) {
    let Some(run_id) = run_id else {
        return (None, Vec::new());
    };
    match read_events_tolerant(&runlog::run_dir(runs, run_id)) {
        Ok(events) => (Some(run_id.to_owned()), tail_lines(&events, lines)),
        Err(error) => {
            notes.push(format!("run `{run_id}` events unreadable: {error}"));
            (Some(run_id.to_owned()), Vec::new())
        }
    }
}

fn header(
    active: Option<&ActivatedConfig>,
    store: Option<&Store>,
    runs: &[RunRow],
    now_ms: u64,
) -> Header {
    Header {
        config_commit: active.map(|active| active.commit.clone()),
        at_ms: now_ms,
        active_leases: store.and_then(|store| store.live_lease_count().ok()),
        running: runs.iter().filter(|row| row.is_running()).count(),
    }
}

/// Loads one frame. `selected` is the run id the detail pane follows;
/// absent a selection it follows the first (most relevant) row.
#[must_use]
pub fn load(roots: &Roots, selected: Option<&str>, detail_lines: usize, now_ms: u64) -> Frame {
    let mut notes = Vec::new();
    let active = read_active(roots, &mut notes);
    let store = open_store(roots, &mut notes);
    let runs = load_runs(&roots.runs, now_ms, &mut notes);
    let detail_run = selected
        .map(str::to_owned)
        .or_else(|| runs.first().map(|row| row.run_id.clone()));
    let (detail_run, detail) =
        load_detail(&roots.runs, detail_run.as_deref(), detail_lines, &mut notes);
    let budgets = load_budgets(active.as_ref(), store.as_ref(), &mut notes);
    Frame {
        header: header(active.as_ref(), store.as_ref(), &runs, now_ms),
        runs,
        budgets,
        detail_run,
        detail,
        notes,
    }
}
