//! The terminal front end: ratatui widgets over the watch view-model.
//! All state reading lives in `bureau::watch`; this file only draws the
//! frame and maps keys.

use std::time::Duration;

use anyhow::Context as _;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};

/// Renamed via alias (import renames are banned): the view-model's
/// `Frame` and ratatui's frame share this file.
type TerminalFrame<'a> = ratatui::Frame<'a>;

use bureau::contract::StepOutcome;
use bureau::runlog::{RunStatus, status_text};
use bureau::watch::{self, BudgetRow, Frame, Roots, RunRow};

use super::{DETAIL_LINES, now_millis};

/// Refresh cadence: one frame per second, keys polled between frames.
const TICK: Duration = Duration::from_secs(1);

/// The selection remembered across refreshes: run id when the table
/// is non-empty, plus the last index as the fallback.
struct Selection {
    run_id: Option<String>,
    index: usize,
}

/// usize → u16 for layout lengths, saturating.
fn len16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

const fn status_style(status: &RunStatus) -> Style {
    match status {
        RunStatus::Running => Style::new().fg(Color::Cyan),
        RunStatus::Finished(StepOutcome::Success) => Style::new().fg(Color::Green),
        RunStatus::Finished(StepOutcome::NoWork) => Style::new().fg(Color::DarkGray),
        RunStatus::Finished(StepOutcome::Failure) => Style::new().fg(Color::Red),
        RunStatus::Finished(StepOutcome::Blocked) => Style::new().fg(Color::Yellow),
    }
}

/// The title: adopted commit, refresh clock, live counts.
fn title_line(frame: &Frame) -> Line<'static> {
    let header = &frame.header;
    let commit = header.config_commit.as_deref().map_or_else(
        || "none".to_owned(),
        |commit| commit.chars().take(12).collect(),
    );
    let leases = header
        .active_leases
        .map_or_else(|| "?".to_owned(), |count| count.to_string());
    Line::from(vec![
        Span::styled("bureau watch", Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(format!(
            " · config {commit} · {} · leases {leases} · running {}",
            watch::clock_text(header.at_ms),
            header.running
        )),
    ])
}

fn header_lines(frame: &Frame) -> Vec<Line<'static>> {
    let hint = Line::from(Span::styled(
        "q quits · up/down selects a run",
        Style::new().fg(Color::DarkGray),
    ));
    let mut lines = vec![title_line(frame), hint];
    lines.extend(frame.notes.iter().map(|note| {
        Line::from(Span::styled(
            format!("note: {note}"),
            Style::new().fg(Color::Yellow),
        ))
    }));
    lines
}

fn run_row(row: &RunRow) -> Row<'static> {
    let cost = row.cost_usd.map_or_else(|| "-".to_owned(), watch::money);
    let cells = [
        row.run_id.clone(),
        row.assignment.clone(),
        row.item.clone(),
        status_text(&row.status),
        row.step.clone(),
        cost,
        watch::age_text(row.age_ms),
    ];
    Row::new(cells).style(status_style(&row.status))
}

fn runs_widget(frame: &Frame) -> Table<'static> {
    let header = Row::new(["ID", "ASSIGNMENT", "ITEM", "STATUS", "STEP", "COST", "AGE"])
        .style(Style::new().fg(Color::DarkGray));
    Table::new(
        frame.runs.iter().map(run_row),
        [
            Constraint::Min(18),
            Constraint::Min(10),
            Constraint::Min(6),
            Constraint::Min(9),
            Constraint::Min(12),
            Constraint::Min(7),
            Constraint::Min(6),
        ],
    )
    .header(header)
    .block(Block::bordered().title("runs"))
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">")
}

fn budget_row(row: &BudgetRow) -> Row<'static> {
    let max_cost = row
        .max_cost_usd
        .map_or_else(|| "-".to_owned(), watch::money);
    let max_hour = row
        .max_runs_hour
        .map_or_else(|| "-".to_owned(), |max| max.to_string());
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
    Row::new(vec![
        row.assignment.clone(),
        format!("{} / {max_cost}", watch::money(row.spent_usd)),
        format!("{} / {max_hour}", row.runs_hour),
        headroom,
    ])
}

fn detail_widget(frame: &Frame, area: Rect) -> Paragraph<'static> {
    let title = frame
        .detail_run
        .as_deref()
        .map_or_else(|| "detail".to_owned(), |run_id| format!("detail: {run_id}"));
    let visible = usize::from(area.height.saturating_sub(2));
    let start = frame.detail.len().saturating_sub(visible);
    let lines: Vec<Line<'static>> = frame.detail[start..]
        .iter()
        .map(|line| Line::from(line.clone()))
        .collect();
    Paragraph::new(lines).block(Block::bordered().title(title))
}

fn draw(terminal: &mut TerminalFrame, frame: &Frame, selected: usize) {
    let areas = Layout::vertical([
        Constraint::Length(len16(2 + frame.notes.len())),
        Constraint::Min(3),
        Constraint::Length(len16(frame.budgets.len() + 3)),
        Constraint::Min(5),
    ])
    .split(terminal.area());
    terminal.render_widget(Paragraph::new(header_lines(frame)), areas[0]);
    let mut state =
        TableState::default().with_selected((!frame.runs.is_empty()).then_some(selected));
    terminal.render_stateful_widget(runs_widget(frame), areas[1], &mut state);
    let budgets: Vec<Row> = frame.budgets.iter().map(budget_row).collect();
    terminal.render_widget(
        Table::new(budgets, [Constraint::Min(14); 4])
            .header(
                Row::new(["ASSIGNMENT", "COST TODAY", "RUNS/HOUR", "HEADROOM"])
                    .style(Style::new().fg(Color::DarkGray)),
            )
            .block(Block::bordered().title("budget (headroom excludes the open-PR limit)")),
        areas[2],
    );
    terminal.render_widget(detail_widget(frame, areas[3]), areas[3]);
}

/// `q`, `Esc`, or `Ctrl-C` ends the loop.
fn quit_key(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn move_selection(selection: &mut Selection, frame: &Frame, code: KeyCode) {
    let last = frame.runs.len().saturating_sub(1);
    let index = match code {
        KeyCode::Up | KeyCode::Char('k') => selection.index.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => (selection.index + 1).min(last),
        _ => return,
    };
    selection.index = index;
    selection.run_id = frame.runs.get(index).map(|row| row.run_id.clone());
}

/// One key press; `true` when the loop should end.
fn handle_key(selection: &mut Selection, frame: &Frame, key: KeyEvent) -> bool {
    if quit_key(&key) {
        return true;
    }
    move_selection(selection, frame, key.code);
    false
}

/// Reloads the frame and re-resolves the selection against it.
fn refresh(roots: &Roots, selection: &mut Selection) -> Frame {
    let frame = watch::load(
        roots,
        selection.run_id.as_deref(),
        DETAIL_LINES,
        now_millis(),
    );
    selection.index =
        watch::resolve_selection(&frame.runs, selection.run_id.as_deref(), selection.index)
            .unwrap_or(0);
    selection.run_id = frame
        .runs
        .get(selection.index)
        .map(|row| row.run_id.clone());
    frame
}

fn drive(terminal: &mut DefaultTerminal, roots: &Roots) -> anyhow::Result<i32> {
    let mut selection = Selection {
        run_id: None,
        index: 0,
    };
    loop {
        let frame = refresh(roots, &mut selection);
        terminal
            .draw(|terminal| draw(terminal, &frame, selection.index))
            .context("drawing the dashboard")?;
        if !event::poll(TICK).context("polling terminal input")? {
            continue;
        }
        if let Event::Key(key) = event::read().context("reading terminal input")? {
            if key.kind != KeyEventKind::Release && handle_key(&mut selection, &frame, key) {
                return Ok(0);
            }
        }
    }
}

/// Runs the dashboard until the user quits. Restores the terminal on
/// every exit path.
///
/// # Errors
/// Propagates terminal setup, draw, and input failures.
pub fn run(roots: &Roots) -> anyhow::Result<i32> {
    let mut terminal = ratatui::try_init().context("initializing the terminal")?;
    let result = drive(&mut terminal, roots);
    // Restoring is best-effort: the loop's own result matters more.
    let _restored = ratatui::try_restore();
    result
}
