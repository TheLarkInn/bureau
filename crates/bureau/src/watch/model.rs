//! Frame types: what one watch screen shows.

use crate::runlog::RunStatus;

/// Header data: the adopted config, the snapshot time, and live counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Adopted config commit, when one is activated.
    pub config_commit: Option<String>,
    /// Snapshot time, milliseconds since the Unix epoch.
    pub at_ms: u64,
    /// Live leases across assignments; `None` when state.db is absent
    /// or unreadable.
    pub active_leases: Option<u32>,
    /// Runs currently running.
    pub running: usize,
}

/// One run's table row.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    /// The run's id.
    pub run_id: String,
    /// The assignment the run belongs to.
    pub assignment: String,
    /// The work item the run targets; `-` when the log predates items.
    pub item: String,
    /// Where the run stands.
    pub status: RunStatus,
    /// Latest step and its state (`work: running`), `starting`, or `-`.
    pub step: String,
    /// Cost so far: the terminal total when finished, else the sum of
    /// per-step measured costs when any step reported one.
    pub cost_usd: Option<f64>,
    /// Wall-clock start, milliseconds since the Unix epoch.
    pub started_at_ms: u64,
    /// Age at snapshot time.
    pub age_ms: u64,
}

impl RunRow {
    /// Whether the run has not finished.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self.status, RunStatus::Running)
    }
}

/// One assignment's budget counters against its configured limits.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetRow {
    /// The assignment.
    pub assignment: String,
    /// Recorded run cost over the last day.
    pub spent_usd: f64,
    /// Configured daily cost ceiling.
    pub max_cost_usd: Option<f64>,
    /// Runs started in the last hour.
    pub runs_hour: u32,
    /// Configured hourly run ceiling.
    pub max_runs_hour: Option<u32>,
    /// Remaining run slots from local counters (the open-PR limit needs
    /// the forge and is excluded); `None` when state.db is unreadable.
    pub headroom: Option<usize>,
}

/// One screen: header, runs, budgets, and the selected run's events.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Header line data.
    pub header: Header,
    /// Run rows, running first then newest first.
    pub runs: Vec<RunRow>,
    /// Budget rows for the adopted config's assignments.
    pub budgets: Vec<BudgetRow>,
    /// The run the detail pane follows.
    pub detail_run: Option<String>,
    /// The detail pane's most recent event lines, oldest first.
    pub detail: Vec<String>,
    /// Degradation notes: every input that could not be read.
    pub notes: Vec<String>,
}

/// Resolves the selected row across refreshes: the remembered run id
/// when it is still present, else the previous index clamped into
/// bounds. `None` when there are no runs.
#[must_use]
pub fn resolve_selection(
    runs: &[RunRow],
    selected: Option<&str>,
    previous: usize,
) -> Option<usize> {
    if runs.is_empty() {
        return None;
    }
    selected
        .and_then(|id| runs.iter().position(|row| row.run_id == id))
        .or_else(|| Some(previous.min(runs.len() - 1)))
}
