//! Binary-level tests for run inspection and pre-spawn failures.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bureau(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(args)
        .output()
        .expect("run bureau")
}

fn bureau_home(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(args)
        .env("BUREAU_HOME", home)
        .output()
        .expect("run bureau")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
}

/// A finished run: started, one step, finished with success.
const EVENTS_FINISHED: &str = r#"{"seq":0,"at_ms":0,"kind":"run_started","data":{"run_id":"r1","assignment":"demo","item":"example/code#42"}}
{"seq":1,"at_ms":1,"kind":"step_started","data":{"step":"work"}}
{"seq":2,"at_ms":2,"kind":"step_finished","data":{"step":"work","outcome":"success"}}
{"seq":3,"at_ms":3,"kind":"run_finished","data":{"outcome":"success"}}
"#;

/// A run still in flight: started, one step begun, never finished.
const EVENTS_UNFINISHED: &str = r#"{"seq":0,"at_ms":0,"kind":"run_started","data":{"run_id":"r2","assignment":"demo","item":"example/code#42"}}
{"seq":1,"at_ms":1,"kind":"step_started","data":{"step":"work"}}
"#;

/// A run whose `run_started` names no item; `retry` cannot help.
const EVENTS_NO_ITEM: &str = r#"{"seq":0,"at_ms":0,"kind":"run_started","data":{"run_id":"r3","assignment":"demo"}}
"#;

fn write_events(dir: &Path, run_id: &str, events: &str) {
    write(dir, &format!("runs/{run_id}/events.jsonl"), events);
}

#[test]
fn run_listing_defaults_to_bureau_home() {
    let dir = TestDir::new("home-list");
    write_events(dir.path(), "r1", EVENTS_FINISHED);
    let output = bureau_home(&["list"], dir.path());
    assert!(output.status.success() && stdout(&output).contains("r1"));
}

#[test]
fn list_on_an_empty_runs_dir_prints_nothing() {
    let dir = TestDir::new("list-empty");
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["list", "--runs", &runs]);
    let got = (output.status.code(), stdout(&output).is_empty());
    assert_eq!(got, (Some(0), true), "{}", stderr(&output));
}

#[test]
fn list_shows_a_finished_run() {
    let dir = TestDir::new("list-finished");
    write_events(dir.path(), "r1", EVENTS_FINISHED);
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["list", "--runs", &runs]);
    let out = stdout(&output);
    let got = (
        output.status.code(),
        out.contains("r1"),
        out.contains("finished(success)"),
        out.contains("demo"),
    );
    assert_eq!(got, (Some(0), true, true, true), "{out}");
}

#[test]
fn show_prints_replayed_state_and_event_tail() {
    let dir = TestDir::new("show-finished");
    write_events(dir.path(), "r1", EVENTS_FINISHED);
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["show", "r1", "--runs", &runs]);
    let out = stdout(&output);
    let got = (
        output.status.code(),
        out.contains("run: r1"),
        out.contains("assignment: demo"),
        out.contains("status: finished(success)"),
        out.contains("work: success"),
        out.contains("#3 run_finished success"),
    );
    assert_eq!(got, (Some(0), true, true, true, true, true), "{out}");
}

#[test]
fn show_on_a_missing_run_exits_2() {
    let dir = TestDir::new("show-missing");
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["show", "ghost", "--runs", &runs]);
    let got = (
        output.status.code(),
        stderr(&output).contains("no such run"),
    );
    assert_eq!(got, (Some(2), true), "{}", stdout(&output));
}

#[test]
fn cancel_on_a_missing_run_exits_2() {
    let dir = TestDir::new("cancel-missing");
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["cancel", "ghost", "--runs", &runs]);
    let got = (
        output.status.code(),
        stderr(&output).contains("no such run"),
    );
    assert_eq!(got, (Some(2), true), "{}", stdout(&output));
}

#[test]
fn cancel_writes_the_marker_for_an_unfinished_run() {
    let dir = TestDir::new("cancel-unfinished");
    write_events(dir.path(), "r2", EVENTS_UNFINISHED);
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["cancel", "r2", "--runs", &runs]);
    let got = (
        output.status.code(),
        dir.path().join("runs/r2/CANCEL").exists(),
        stdout(&output).contains("r2"),
    );
    assert_eq!(got, (Some(0), true, true), "{}", stderr(&output));
}

#[test]
fn cancel_on_a_finished_run_exits_1() {
    let dir = TestDir::new("cancel-finished");
    write_events(dir.path(), "r1", EVENTS_FINISHED);
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["cancel", "r1", "--runs", &runs]);
    let got = (
        output.status.code(),
        stdout(&output).contains("already finished"),
        dir.path().join("runs/r1/CANCEL").exists(),
    );
    assert_eq!(got, (Some(1), true, false), "{}", stderr(&output));
}

#[test]
fn retry_on_a_run_without_an_item_exits_2() {
    let dir = TestDir::new("retry-no-item");
    write_events(dir.path(), "r3", EVENTS_NO_ITEM);
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["retry", "r3", "--runs", &runs]);
    let got = (
        output.status.code(),
        stderr(&output).contains("no work item"),
    );
    assert_eq!(got, (Some(2), true), "{}", stdout(&output));
}

#[test]
fn retry_on_a_missing_run_exits_2() {
    let dir = TestDir::new("retry-missing");
    let runs = dir.path().join("runs").to_string_lossy().into_owned();
    let output = bureau(&["retry", "ghost", "--runs", &runs]);
    let got = (
        output.status.code(),
        stderr(&output).contains("no such run"),
    );
    assert_eq!(got, (Some(2), true), "{}", stdout(&output));
}
