//! Plain-text rendering of a frame: the piped-output snapshot, and the
//! reference the terminal front end's layout mirrors.

use super::model::{BudgetRow, Frame, Header, RunRow};
use crate::runlog::status_text;

/// `$1.23`.
#[must_use]
pub fn money(usd: f64) -> String {
    format!("${usd:.2}")
}

/// UTC wall time `HH:MM:SSZ` — the header's refresh stamp. A date would
/// need a calendar; the dashboard cares about the clock.
#[must_use]
pub fn clock_text(at_ms: u64) -> String {
    let seconds = (at_ms / 1000) % 86_400;
    format!(
        "{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

/// Compact age: `42s`, `4m12s`, `3h04m`, `2d`.
#[must_use]
pub fn age_text(age_ms: u64) -> String {
    let seconds = age_ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else if seconds < 86_400 {
        format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

/// One header line: adopted commit, refresh clock, live counts.
fn header_line(header: &Header) -> String {
    let commit = header.config_commit.as_deref().map_or_else(
        || "none".to_owned(),
        |commit| commit.chars().take(12).collect(),
    );
    let leases = header
        .active_leases
        .map_or_else(|| "?".to_owned(), |count| count.to_string());
    format!(
        "bureau watch · config {commit} · {} · leases {leases} · running {}",
        clock_text(header.at_ms),
        header.running
    )
}

/// Column widths fitting both the headers and every row.
fn widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.len());
        }
    }
    widths
}

/// Cells padded to the column widths, joined by two spaces.
fn pad_row(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell:<width$}"))
        .collect::<Vec<_>>()
        .join("  ")
}

/// A titled two-space-indented table; `none` when there are no rows.
fn section(title: &str, headers: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let mut lines = vec![title.to_owned()];
    if rows.is_empty() {
        lines.push("  none".to_owned());
        return lines;
    }
    let widths = widths(headers, rows);
    let owned: Vec<String> = headers.iter().map(|header| (*header).to_owned()).collect();
    lines.push(pad_row(&owned, &widths));
    lines.extend(rows.iter().map(|row| pad_row(row, &widths)));
    lines
}

fn run_cells(row: &RunRow) -> Vec<String> {
    vec![
        row.run_id.clone(),
        row.assignment.clone(),
        row.item.clone(),
        status_text(&row.status),
        row.step.clone(),
        row.cost_usd.map_or_else(|| "-".to_owned(), money),
        age_text(row.age_ms),
    ]
}

/// `used / max`, the max side becoming `-` when unset.
fn ratio(used: &str, max: Option<String>) -> String {
    format!("{} / {}", used, max.unwrap_or_else(|| "-".to_owned()))
}

fn budget_cells(row: &BudgetRow) -> Vec<String> {
    let headroom = row.headroom.map_or_else(
        || "?".to_owned(),
        |headroom| {
            if headroom == usize::MAX {
                "unlimited".to_owned()
            } else {
                headroom.to_string()
            }
        },
    );
    vec![
        row.assignment.clone(),
        ratio(&money(row.spent_usd), row.max_cost_usd.map(money)),
        ratio(
            &row.runs_hour.to_string(),
            row.max_runs_hour.map(|max| max.to_string()),
        ),
        headroom,
    ]
}

/// The detail pane: which run, then its latest event lines.
fn detail_section(frame: &Frame) -> Vec<String> {
    let title = frame.detail_run.as_deref().map_or_else(
        || "detail: no run selected".to_owned(),
        |run_id| format!("detail: {run_id} (latest events)"),
    );
    let mut lines = vec![title];
    if frame.detail.is_empty() {
        lines.push("  none".to_owned());
    }
    lines.extend(frame.detail.iter().map(|line| format!("  {line}")));
    lines
}

/// Renders one frame as plain-text lines.
#[must_use]
pub fn render_plain(frame: &Frame) -> Vec<String> {
    let mut lines = vec![header_line(&frame.header)];
    lines.extend(frame.notes.iter().map(|note| format!("note: {note}")));
    lines.push(String::new());
    let runs: Vec<Vec<String>> = frame.runs.iter().map(run_cells).collect();
    lines.extend(section(
        "runs:",
        &["ID", "ASSIGNMENT", "ITEM", "STATUS", "STEP", "COST", "AGE"],
        &runs,
    ));
    lines.push(String::new());
    let budgets: Vec<Vec<String>> = frame.budgets.iter().map(budget_cells).collect();
    lines.extend(section(
        "budget (cost today, runs/hour, headroom excl. open-PR limit):",
        &["ASSIGNMENT", "COST", "RUNS/HR", "HEADROOM"],
        &budgets,
    ));
    lines.push(String::new());
    lines.extend(detail_section(frame));
    lines
}
