//! `bureau watch` projection tests over fixture run dirs, a fixture
//! state.db, and a fixture adopted config: offline, no network, no
//! model calls (DESIGN.md sections 7 and 12).

pub mod watch_support;

use bureau::contract::StepOutcome;
use bureau::runlog::RunStatus;
use bureau::watch;
use watch_support::{
    TestDir, write_active, write_chatty_run, write_finished_run, write_running_run, write_state_db,
};

/// A torn state cache and a torn final event line, as a daemon kill
/// mid-append leaves them, plus the strays a runs dir collects.
fn corrupt(run_dir: &std::path::Path) {
    std::fs::write(run_dir.join("state.json"), b"{\"run_id\": partial").expect("torn cache");
    std::fs::OpenOptions::new()
        .append(true)
        .open(run_dir.join("events.jsonl"))
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"{\"seq\": 4, part"))
        .expect("torn tail");
    let runs = run_dir.parent().expect("runs root");
    std::fs::create_dir_all(runs.join("empty-dir")).expect("empty dir");
    std::fs::write(runs.join("stray-file"), b"x").expect("stray file");
}

#[test]
fn empty_home_renders_an_empty_dashboard() {
    let dir = TestDir::new("empty");
    let frame = watch::load(&dir.roots(), None, 16, 1_000_000);
    let empty = (
        frame.runs.is_empty(),
        frame.budgets.is_empty(),
        frame.detail.is_empty(),
        frame.header.active_leases,
    );
    assert_eq!(empty, (true, true, true, None));
    let text = watch::render_plain(&frame).join("\n");
    let rendered =
        text.contains("runs:") && text.contains("none") && frame.header.config_commit.is_none();
    assert!(rendered, "{text}");
}

#[test]
fn settled_run_reads_its_state_cache() {
    let dir = TestDir::new("cached");
    let runs = dir.roots().runs;
    write_finished_run(&runs, "demo-1000-aa", 1_000, 2.5);
    std::fs::remove_file(runs.join("demo-1000-aa").join("events.jsonl")).expect("remove log");
    let frame = watch::load(&dir.roots(), None, 16, 11_000);
    let row = frame.runs.first().expect("one row");
    let fields = (
        row.status.clone(),
        row.step.as_str(),
        row.cost_usd,
        row.age_ms,
    );
    assert_eq!(
        fields,
        (
            RunStatus::Finished(StepOutcome::Success),
            "work: success",
            Some(2.5),
            10_000
        ),
        "the row comes from the cache alone"
    );
}

#[test]
fn running_run_replays_its_log() {
    let dir = TestDir::new("running");
    write_running_run(&dir.roots().runs, "demo-2000-bb", 5_000);
    let frame = watch::load(&dir.roots(), None, 16, 10_000);
    let row = frame.runs.first().expect("one row");
    let fields = (
        row.is_running(),
        row.step.as_str(),
        row.cost_usd,
        row.age_ms,
    );
    assert_eq!(fields, (true, "work: running", None, 5_000));
    let detail = (frame.detail_run.as_deref(), frame.detail.len());
    assert_eq!(
        detail,
        (Some("demo-2000-bb"), 2),
        "detail follows the only run"
    );
}

#[test]
fn torn_inputs_degrade_to_notes_not_panics() {
    let dir = TestDir::new("torn");
    let runs = dir.roots().runs;
    write_finished_run(&runs, "demo-1000-aa", 1_000, 1.0);
    corrupt(&runs.join("demo-1000-aa"));
    let frame = watch::load(&dir.roots(), None, 16, 5_000);
    let replayed = frame.runs.first().expect("torn inputs still replay");
    let outcome = (
        replayed.status.clone(),
        frame
            .notes
            .iter()
            .any(|n| n.contains("1 run(s) unreadable")),
        frame
            .detail
            .last()
            .expect("detail")
            .contains("run_finished"),
        frame.runs.len(),
    );
    assert_eq!(
        outcome,
        (RunStatus::Finished(StepOutcome::Success), true, true, 1),
        "torn cache replays, torn tail drops, empty dir is a note"
    );
}

#[test]
fn budgets_pair_config_limits_with_store_counters() {
    let dir = TestDir::new("budget");
    write_active(dir.path(), "abc1234567890def");
    write_state_db(dir.path());
    let frame = watch::load(&dir.roots(), None, 16, 1_000_000);
    let header = (
        frame.header.config_commit.as_deref(),
        frame.header.active_leases,
    );
    assert_eq!(header, (Some("abc1234567890def"), Some(1)));
    let row = frame.budgets.first().expect("one budget row");
    let fields = (row.spent_usd, row.runs_hour, row.headroom);
    assert_eq!(
        fields,
        (6.0, 2, Some(1)),
        "headroom: concurrent limit binds"
    );
}

#[test]
fn unreadable_state_db_degrades_to_notes() {
    let dir = TestDir::new("baddb");
    write_active(dir.path(), "abc1234567890def");
    std::fs::write(dir.roots().state, b"not a sqlite database").expect("garbage db");
    let frame = watch::load(&dir.roots(), None, 16, 1_000_000);
    let degraded = (
        frame.budgets.is_empty(),
        frame.header.active_leases,
        frame.notes.iter().any(|n| n.contains("budget for `demo`")),
    );
    assert_eq!(degraded, (true, None, true));
}

#[test]
fn detail_tails_the_selected_run() {
    let dir = TestDir::new("detail");
    let runs = dir.roots().runs;
    write_running_run(&runs, "demo-1000-aa", 1_000);
    write_chatty_run(&runs, "demo-2000-bb", 2_000);
    let frame = watch::load(&dir.roots(), Some("demo-2000-bb"), 3, 9_000);
    let detail = (
        frame.detail_run.as_deref(),
        frame.detail.len(),
        frame.detail[0].starts_with("#4 "),
    );
    assert_eq!(
        detail,
        (Some("demo-2000-bb"), 3, true),
        "last three, oldest first"
    );
}
