//! `bureau watch` projection tests over fixture run dirs, a fixture
//! state.db, and a fixture adopted config: offline, no network, no
//! model calls (DESIGN.md sections 7 and 12).

pub mod watch_support;

use bureau::contract::StepOutcome;
use bureau::runlog::RunStatus;
use bureau::state::Store;
use bureau::watch;
use watch_support::{
    TestDir, load_read_only, write_active, write_chatty_run, write_finished_run, write_running_run,
    write_state_db,
};

const INVALID_ACCOUNTING: [(&str, &str, Option<u32>); 4] = [
    (
        "DROP TABLE run_admissions",
        "state database unreadable:",
        None,
    ),
    (
        "PRAGMA user_version = 0",
        "state database unreadable:",
        None,
    ),
    (
        "PRAGMA user_version = 2",
        "state database unreadable:",
        None,
    ),
    (
        "PRAGMA user_version = 0; DROP TABLE run_admissions; DROP TABLE runs",
        "budget for `demo` unreadable:",
        Some(1),
    ),
];

fn observed_budget(roots: &watch::Roots) -> (Option<u32>, f64, u32, Option<usize>) {
    let frame = load_read_only(roots);
    let row = frame.budgets.first().expect("one budget row");
    (
        frame.header.active_leases,
        row.spent_usd,
        row.runs_hour,
        row.headroom,
    )
}

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
    let frame = load_read_only(&dir.roots());
    let header = (
        frame.header.config_commit.as_deref(),
        frame.header.active_leases,
    );
    assert_eq!(header, (Some("abc1234567890def"), Some(1)));
    let row = frame.budgets.first().expect("one budget row");
    let fields = (row.spent_usd, row.runs_hour, row.headroom);
    assert_eq!(
        fields,
        (6.0, 3, Some(1)),
        "two completions plus one admission; concurrent and hourly limits both bind"
    );
}

#[test]
fn unreadable_state_db_degrades_to_notes() {
    let dir = TestDir::new("baddb");
    write_active(dir.path(), "abc1234567890def");
    std::fs::write(dir.roots().state, b"not a sqlite database").expect("garbage db");
    let frame = load_read_only(&dir.roots());
    let degraded = (
        frame.budgets.is_empty(),
        frame.header.active_leases,
        frame
            .notes
            .iter()
            .any(|n| n.starts_with("state database unreadable:")),
    );
    assert_eq!(degraded, (true, None, true));
}

#[test]
fn released_admission_remains_charged_when_terminal_cost_is_projected() {
    let dir = TestDir::new("released-budget");
    write_active(dir.path(), "abc1234567890def");
    write_state_db(dir.path());
    let roots = dir.roots();
    let store = Store::open(&roots.state).expect("writer");
    let live = observed_budget(&roots);
    store
        .release("demo", "42")
        .expect("release unprojected admission");
    let released = observed_budget(&roots);
    store
        .record_run("42", "demo", 2.0)
        .expect("project terminal cost");
    let projected = observed_budget(&roots);
    assert_eq!(
        [live, released, projected],
        [
            (Some(1), 6.0, 3, Some(1)),
            (Some(0), 6.0, 3, Some(1)),
            (Some(0), 8.0, 3, Some(1)),
        ],
        "release restores concurrency, not the rate budget; projection charges cost only"
    );
}

#[test]
fn invalid_accounting_never_projects_free_capacity_or_repairs_evidence() {
    for (sql, note, leases) in INVALID_ACCOUNTING {
        let dir = TestDir::new("invalid-accounting");
        write_active(dir.path(), "abc1234567890def");
        write_state_db(dir.path());
        let roots = dir.roots();
        rusqlite::Connection::open(&roots.state)
            .expect("fixture connection")
            .execute_batch(sql)
            .expect("inject invalid accounting");
        let frame = load_read_only(&roots);
        let unknown = (
            frame.budgets.is_empty(),
            frame.header.active_leases,
            frame.notes.iter().any(|n| n.starts_with(note)),
        );
        assert_eq!(unknown, (true, leases, true), "{sql}");
    }
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
