//! Binary-level tests for `run`, `list`, `show`, `cancel`, and `retry`,
//! driven through the built `bureau` binary with temp dirs.
//!
//! A full `run` success path needs a forge to query and a pipeline to
//! execute; that end-to-end coverage belongs to the reference-e2e tests,
//! not the binary test. Here every path stops before any spawn.

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

const MINIMAL_REPO: &str = r"
repos:
  code:
    url: https://github.com/example/code
    forge: github
    access: push
    credential: gh-main
";

const MINIMAL_ROLE: &str = r"
name: worker
agent: agents/worker.md
adapter: fake
permissions: [repo:read, repo:write, pr:write]
min_trust: untrusted
";

const MINIMAL_PIPELINE: &str = r#"
name: fix-failing-test
steps:
  - name: work
    type: deterministic
    run: "true"
    next: done
"#;

const MINIMAL_ASSIGNMENT: &str = r#"
name: demo
work:
  forge: github
  source: "example/code"
  filter: "label:agent-eligible"
repos: [code]
pipeline: fix-failing-test
role: worker
verify: "make test"
branch_prefix: runner/
limits:
  max_concurrent: 1
  max_runs_per_hour: 4
  max_runs_per_day: 20
  max_open_prs: 3
  max_cost_per_day_usd: 10
"#;

fn write_minimal_config(dir: &Path) {
    write(dir, "repos.yaml", MINIMAL_REPO);
    write(dir, "roles/worker.yaml", MINIMAL_ROLE);
    write(dir, "assignments/demo.yaml", MINIMAL_ASSIGNMENT);
    write(dir, "pipelines/fix-failing-test.yaml", MINIMAL_PIPELINE);
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
fn run_rejects_an_unknown_pipeline() {
    let dir = TestDir::new("run-unknown-pipeline");
    write_minimal_config(dir.path());
    let config = dir.path().to_string_lossy().into_owned();
    let output = bureau(&["run", "ghost", "--item", "42", "--config", &config]);
    let got = (output.status.code(), stderr(&output).contains("ghost"));
    assert_eq!(got, (Some(2), true), "{}", stderr(&output));
}

#[test]
fn run_fails_before_spawn_when_a_credential_is_missing() {
    let dir = TestDir::new("run-missing-credential");
    write_minimal_config(dir.path());
    let config = dir.path().to_string_lossy().into_owned();
    let output = Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args([
            "run",
            "fix-failing-test",
            "--item",
            "42",
            "--config",
            &config,
        ])
        .current_dir(dir.path())
        .env_remove("BUREAU_CREDENTIAL_GH_MAIN")
        .env_remove("BUREAU_CREDENTIALS_DIR")
        .output()
        .expect("run bureau");
    let got = (
        output.status.code(),
        stderr(&output).contains("gh-main"),
        dir.path().join("state.db").exists(),
    );
    assert_eq!(got, (Some(2), true, false), "{}", stderr(&output));
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
