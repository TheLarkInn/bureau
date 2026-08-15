//! Layer 3 run-log tests (DESIGN.md section 7): append, sequence,
//! scrub-on-write, and replay reconstructing identical state after
//! `state.json` is deleted.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::contract::StepOutcome;
use bureau::process::{REDACTED, Secret};
use bureau::runlog::{
    self, EventKind, RunLog, RunState, RunStatus, StepRecord, run_finished, run_started,
    step_finished, step_started,
};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// The tests' clock: real millis since the Unix epoch.
fn test_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

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

/// Appends a representative run and closes the log.
fn write_run(runs_dir: &Path, run_id: &str, secrets: &[Secret]) -> PathBuf {
    let mut log = RunLog::create(runs_dir, run_id, secrets, test_clock).expect("create");
    log.append(
        EventKind::RunStarted,
        run_started(run_id, "fix-flaky-tests"),
    )
    .expect("append");
    log.append(EventKind::StepStarted, step_started("propose"))
        .expect("append");
    log.append(
        EventKind::StepFinished,
        step_finished("propose", StepOutcome::Success),
    )
    .expect("append");
    log.append(EventKind::RunFinished, run_finished(StepOutcome::Success))
        .expect("append");
    let dir = log.dir().to_path_buf();
    log.close().expect("close");
    dir
}

fn expected_state(run_id: &str) -> RunState {
    RunState {
        run_id: run_id.to_owned(),
        assignment: "fix-flaky-tests".to_owned(),
        steps: vec![StepRecord {
            step: "propose".to_owned(),
            outcome: Some(StepOutcome::Success),
        }],
        status: RunStatus::Finished(StepOutcome::Success),
    }
}

#[test]
fn events_are_sequence_numbered() {
    let dir = TestDir::new("seq");
    let mut log = RunLog::create(dir.path(), "run-1", &[], test_clock).expect("create");
    let first = log
        .append(EventKind::RunStarted, run_started("run-1", "a"))
        .expect("append");
    let second = log
        .append(EventKind::RunFinished, run_finished(StepOutcome::NoWork))
        .expect("append");
    assert_eq!((first, second), (0, 1));
    log.close().expect("close");
}

#[test]
fn replay_rebuilds_identical_state_after_cache_is_deleted() {
    let dir = TestDir::new("replay");
    let run = write_run(dir.path(), "run-1", &[]);
    let state = runlog::replay_state(&run).expect("replay");
    assert_eq!(state, expected_state("run-1"));
    runlog::write_state_cache(&run, &state).expect("write cache");
    let cached = std::fs::read(run.join(runlog::STATE_FILE)).expect("read cache");
    std::fs::remove_file(run.join(runlog::STATE_FILE)).expect("delete cache");
    let rebuilt = runlog::replay_state(&run).expect("replay after cache loss");
    assert_eq!(rebuilt, state);
    runlog::write_state_cache(&run, &rebuilt).expect("rewrite cache");
    assert_eq!(
        std::fs::read(run.join(runlog::STATE_FILE)).expect("read cache"),
        cached
    );
}

#[test]
fn secrets_are_scrubbed_on_write() {
    let dir = TestDir::new("scrub");
    let secret = "hunter2hunter2";
    let mut log =
        RunLog::create(dir.path(), "run-1", &[Secret::new(secret)], test_clock).expect("create");
    log.append(
        EventKind::StepStarted,
        serde_json::json!({ "step": secret }),
    )
    .expect("append");
    log.close().expect("close");
    let raw =
        std::fs::read_to_string(dir.path().join("run-1").join(runlog::EVENTS_FILE)).expect("read");
    assert!(!raw.contains(secret), "secret in log: {raw}");
    assert!(raw.contains(REDACTED), "no redaction marker: {raw}");
}

#[test]
fn a_run_id_is_used_exactly_once() {
    let dir = TestDir::new("dupe");
    write_run(dir.path(), "run-1", &[]);
    let second = RunLog::create(dir.path(), "run-1", &[], test_clock);
    assert!(second.is_err());
}

#[test]
fn read_events_drops_a_torn_final_line() {
    let dir = TestDir::new("torn");
    let run = write_run(dir.path(), "run-1", &[]);
    let intact = runlog::read_events(&run).expect("intact log reads");
    // A kill mid-append: partial JSON, no trailing newline, never closed.
    let mut file = OpenOptions::new()
        .append(true)
        .open(run.join(runlog::EVENTS_FILE))
        .expect("open for append");
    file.write_all(b"{\"seq\":4,\"at_ms\":0,\"kind\":\"outp")
        .expect("torn append");
    drop(file);
    assert_eq!(runlog::read_events(&run).expect("torn tail reads"), intact);
}

#[test]
fn replay_rejects_a_corrupt_line() {
    let dir = TestDir::new("corrupt");
    let run = write_run(dir.path(), "run-1", &[]);
    let path = run.join(runlog::EVENTS_FILE);
    let raw = std::fs::read_to_string(&path).expect("read");
    let corrupt = raw.replacen('\n', "\n{ not json\n", 1);
    std::fs::write(&path, corrupt).expect("corrupt");
    assert!(runlog::read_events(&run).is_err());
}

#[test]
fn replay_rejects_a_log_without_run_started() {
    let dir = TestDir::new("nostart");
    let mut log = RunLog::create(dir.path(), "run-1", &[], test_clock).expect("create");
    log.append(EventKind::StepStarted, step_started("propose"))
        .expect("append");
    let run = log.dir().to_path_buf();
    log.close().expect("close");
    assert!(runlog::replay_state(&run).is_err());
}
