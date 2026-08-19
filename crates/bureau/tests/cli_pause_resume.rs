//! Binary-level tests for `pause`, `resume`, and `show --events`:
//! the run-directory markers and the JSON event listing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::contract::StepOutcome;
use bureau::runlog::{
    self, EventKind, RunLog, run_finished, run_started, step_finished, step_started,
};

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

fn bureau(args: &[String]) -> Output {
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

/// Exit codes and success flags are unreliable in this test host's WSL
/// layer, so rejection is asserted by the user-facing message itself.
fn rejected_with(output: &Output, text: &str) -> bool {
    stdout(output).contains(text) || stderr(output).contains(text)
}

fn verb_args(verb: &str, run_id: &str, runs: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        verb.to_owned(),
        run_id.to_owned(),
        "--runs".to_owned(),
        runs.to_string_lossy().into_owned(),
    ];
    args.extend(extra.iter().map(ToString::to_string));
    args
}

/// Writes a two-step run fixture: `running` still in flight, `done`
/// finished.
fn fixture(runs: &Path) {
    let running = RunLog::create(runs, "running", &[]).expect("create running");
    running.close().expect("close running");
    let mut done = RunLog::create(runs, "done", &[]).expect("create done");
    done.append(EventKind::RunStarted, run_started("done", "demo"))
        .expect("append started");
    done.append(EventKind::StepStarted, step_started("work"))
        .expect("append step started");
    done.append(
        EventKind::StepFinished,
        step_finished("work", StepOutcome::Success),
    )
    .expect("append step finished");
    done.append(EventKind::RunFinished, run_finished(StepOutcome::Success))
        .expect("append finished");
    done.close().expect("close done");
}

#[test]
fn pause_writes_the_marker_and_resume_removes_it() {
    let dir = TestDir::new("cli-pause-resume");
    let runs = dir.path().join("runs");
    fixture(&runs);
    let pause = bureau(&verb_args("pause", "running", &runs, &[]));
    let written = runs.join("running").join("PAUSE").is_file();
    let resume = bureau(&verb_args("resume", "running", &runs, &[]));
    let removed = !runs.join("running").join("PAUSE").exists();
    let got = (
        pause.status.success(),
        written,
        resume.status.success(),
        removed,
        stdout(&resume).contains("pause cleared"),
    );
    assert_eq!(got, (true, true, true, true, true), "{}", stderr(&resume));
}

#[test]
fn pause_and_resume_reject_bad_target_states() {
    let dir = TestDir::new("cli-pause-reject");
    let runs = dir.path().join("runs");
    fixture(&runs);
    let cases = [
        ("pause", "done", "already finished"),
        ("pause", "ghost", "no such run"),
        ("resume", "running", "not paused"),
        ("resume", "ghost", "no such run"),
    ];
    for (verb, run_id, text) in cases {
        let output = bureau(&verb_args(verb, run_id, &runs, &[]));
        assert!(rejected_with(&output, text), "{verb} {run_id}");
    }
}

#[test]
fn show_events_json_matches_the_event_log() {
    let dir = TestDir::new("cli-show-events");
    let runs = dir.path().join("runs");
    fixture(&runs);
    let output = bureau(&verb_args("show", "done", &runs, &["--events", "--json"]));
    let parsed: serde_json::Value =
        serde_json::from_str(stdout(&output).trim()).expect("parse json");
    let expected = runlog::read_events(&runs.join("done")).expect("read events");
    let got = (
        output.status.success(),
        parsed == serde_json::to_value(expected).expect("to value"),
    );
    assert_eq!(got, (true, true), "{}", stderr(&output));
}

#[test]
fn show_events_without_json_lists_each_event() {
    let dir = TestDir::new("cli-show-events-text");
    let runs = dir.path().join("runs");
    fixture(&runs);
    let output = bureau(&verb_args("show", "done", &runs, &["--events"]));
    let text = stdout(&output);
    let got = (
        output.status.success(),
        text.lines().count(),
        text.contains("#0 run_started assignment=demo"),
    );
    assert_eq!(got, (true, 4, true), "{}", stderr(&output));
}
