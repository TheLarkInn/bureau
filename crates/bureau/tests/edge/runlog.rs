//! Run-log adversarial edges (DESIGN.md layer 3): close semantics,
//! blank lines, duplicate sequence numbers, and a mid-run state cache.

use std::path::{Path, PathBuf};

use bureau::contract::StepOutcome;
use bureau::runlog::{self, EventKind, RunLog, RunStatus, run_finished, run_started, step_started};

use super::testdir::TestDir;

/// The tests' clock: real millis since the Unix epoch.
fn test_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A closed log holding `run_started` and `run_finished` events.
fn finished_run(runs_dir: &Path) -> PathBuf {
    let mut log = RunLog::create(runs_dir, "run-1", &[], test_clock).expect("create");
    log.append(EventKind::RunStarted, run_started("run-1", "a"))
        .expect("append");
    log.append(EventKind::RunFinished, run_finished(StepOutcome::Success))
        .expect("append");
    let dir = log.dir().to_path_buf();
    log.close().expect("close");
    dir
}

#[test]
fn close_makes_append_after_close_unrepresentable() {
    // RunLog::close(self) takes the log by value, so appending after
    // close is a compile error — misuse is impossible by construction,
    // and there is no runtime case to test. This documents that proof.
    let dir = TestDir::new("closed");
    let log = RunLog::create(dir.path(), "run-1", &[], test_clock).expect("create");
    log.close().expect("close");
    let events = runlog::read_events(&dir.path().join("run-1")).expect("read");
    assert!(events.is_empty());
}

#[test]
fn blank_lines_between_events_are_tolerated() {
    let dir = TestDir::new("blank");
    let run = finished_run(dir.path());
    let original = runlog::read_events(&run).expect("read");
    let text = std::fs::read_to_string(run.join(runlog::EVENTS_FILE)).expect("raw");
    let mut padded = String::new();
    for line in text.lines() {
        padded.push_str("  \n\n");
        padded.push_str(line);
        padded.push('\n');
    }
    std::fs::write(run.join(runlog::EVENTS_FILE), padded).expect("rewrite");
    let reread = runlog::read_events(&run).expect("read padded");
    assert_eq!(reread, original);
    assert!(runlog::replay_state(&run).is_ok());
}

#[test]
fn replay_does_not_validate_sequence_numbers() {
    let dir = TestDir::new("dupseq");
    let mut log = RunLog::create(dir.path(), "run-1", &[], test_clock).expect("create");
    log.append(EventKind::RunStarted, run_started("run-1", "a"))
        .expect("append");
    log.append(EventKind::StepStarted, step_started("propose"))
        .expect("append");
    let run = log.dir().to_path_buf();
    log.close().expect("close");
    let text = std::fs::read_to_string(run.join(runlog::EVENTS_FILE)).expect("raw");
    let lines: Vec<&str> = text.lines().collect();
    let doubled = format!("{}\n{}\n", lines[..2].join("\n"), lines[1]);
    std::fs::write(run.join(runlog::EVENTS_FILE), doubled).expect("rewrite");
    // Replay folds events in file order and never looks at `seq`: the
    // duplicated step_started lands as a phantom second step record.
    // Safe only because the log has exactly one writer — seq is for
    // readers, not integrity (DESIGN.md layer 3).
    let state = runlog::replay_state(&run).expect("replay");
    let steps: Vec<&str> = state.steps.iter().map(|s| s.step.as_str()).collect();
    assert_eq!(steps, ["propose", "propose"]);
}

#[test]
fn a_state_cache_written_mid_run_is_never_consulted() {
    let dir = TestDir::new("midcache");
    let mut log = RunLog::create(dir.path(), "run-1", &[], test_clock).expect("create");
    log.append(EventKind::RunStarted, run_started("run-1", "a"))
        .expect("append");
    let run = log.dir().to_path_buf();
    let running = runlog::replay_state(&run).expect("replay running");
    runlog::write_state_cache(&run, &running).expect("cache");
    log.append(EventKind::RunFinished, run_finished(StepOutcome::Success))
        .expect("append");
    log.close().expect("close");
    // The cache on disk still says Running, but replay reads only
    // events.jsonl — the log is the source of truth (DESIGN.md layer 3),
    // so a cache written for an unfinished run is stale, never wrong.
    let state = runlog::replay_state(&run).expect("replay");
    let cache = std::fs::read_to_string(run.join(runlog::STATE_FILE)).expect("cache file");
    assert_eq!(state.status, RunStatus::Finished(StepOutcome::Success));
    assert!(cache.contains("\"state\": \"running\""), "{cache}");
}
