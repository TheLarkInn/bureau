use std::fs::File;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::json;

use super::create_events_with;
use crate::process::{REDACTED, Secret};
use crate::runlog::{EVENTS_FILE, Event, EventKind, RunLog, gist, kind_name, read_events_tolerant};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "runlog-create-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&path).expect("fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pipeline_construction_keeps_worktree_and_artifacts() {
    let dir = Directory::new();
    let log = RunLog::create(dir.path(), "pipeline", &[]).expect("pipeline log");
    let actual = (
        log.dir().join("wt").is_dir(),
        log.dir().join("artifacts").is_dir(),
        log.dir().join(EVENTS_FILE).is_file(),
    );
    assert_eq!(actual, (true, true, true));
    log.close().expect("close");
}

#[test]
fn events_only_construction_creates_no_pipeline_directories() {
    let dir = Directory::new();
    let log = RunLog::create_events(dir.path(), "receipt", &[]).expect("events log");
    let names: Vec<_> = std::fs::read_dir(log.dir())
        .expect("directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(names, [EVENTS_FILE]);
    log.close().expect("close");
}

#[test]
fn both_constructors_refuse_an_existing_log() {
    let dir = Directory::new();
    let log = RunLog::create_events(dir.path(), "receipt", &[]).expect("events log");
    log.close().expect("close");
    let actual = (
        RunLog::create_events(dir.path(), "receipt", &[])
            .err()
            .map(|error| error.kind()),
        RunLog::create(dir.path(), "receipt", &[])
            .err()
            .map(|error| error.kind()),
    );
    assert_eq!(
        actual,
        (
            Some(io::ErrorKind::AlreadyExists),
            Some(io::ErrorKind::AlreadyExists)
        )
    );
}

#[test]
fn events_only_writer_scrubs_before_returning() {
    let dir = Directory::new();
    let secret = "test-only-cloud-secret";
    let mut log =
        RunLog::create_events(dir.path(), "receipt", &[Secret::new(secret)]).expect("events log");
    log.append(EventKind::GitHubCloud, json!({"message": secret}))
        .expect("append");
    let bytes = std::fs::read_to_string(log.dir().join(EVENTS_FILE)).expect("raw log");
    let events = read_events_tolerant(log.dir()).expect("events");
    assert_eq!(
        (bytes.contains(secret), events[0].data["message"].as_str()),
        (false, Some(REDACTED))
    );
    log.close().expect("close");
}

#[test]
fn creation_propagates_file_and_each_directory_sync_failure() {
    for fail_at in 0..4 {
        let dir = Directory::new();
        let mut calls = 0;
        let result = create_events_with(&dir.path().join("runs"), "receipt", &[], |file| {
            let position = calls;
            calls += 1;
            if position == fail_at {
                return Err(io::Error::other("injected sync failure"));
            }
            file.sync_all()
        });
        assert_eq!((result.is_err(), calls), (true, fail_at + 1));
    }
}

#[test]
fn existing_run_directory_is_synced_after_the_file() {
    let dir = Directory::new();
    std::fs::create_dir(dir.path().join("receipt")).expect("existing run directory");
    let mut calls = 0;
    let log = create_events_with(dir.path(), "receipt", &[], |file| {
        calls += 1;
        file.sync_all()
    })
    .expect("events log");
    assert_eq!(calls, 2);
    log.close().expect("close");
}

#[test]
fn write_failure_does_not_advance_the_sequence() {
    let dir = Directory::new();
    let mut log = RunLog::create_events(dir.path(), "receipt", &[]).expect("events log");
    log.writer = BufWriter::new(File::open(log.dir().join(EVENTS_FILE)).expect("read-only file"));
    let error = log.append(EventKind::GitHubCloud, json!({}));
    assert_eq!((error.is_err(), log.next_seq), (true, 0));
}

#[cfg(unix)]
#[test]
fn append_reports_sync_failure_after_successful_write() {
    let dir = Directory::new();
    let mut log = RunLog::create_events(dir.path(), "receipt", &[]).expect("events log");
    let (_read, write) = nix::unistd::pipe().expect("local pipe");
    log.writer = BufWriter::new(File::from(write));
    let result = log.append(EventKind::GitHubCloud, json!({}));
    assert_eq!((result.is_err(), log.next_seq), (true, 0));
}

#[test]
fn cloud_kind_and_gist_do_not_claim_pipeline_success() {
    let event = Event {
        seq: 0,
        at_ms: 1,
        kind: EventKind::GitHubCloud,
        data: json!({"record": {"kind": "accepted"}}),
    };
    let encoded = serde_json::to_value(&event).expect("event JSON");
    assert_eq!(
        (
            encoded["kind"].as_str(),
            kind_name(event.kind),
            gist(&event)
        ),
        (
            Some("github_cloud"),
            "github_cloud",
            "github_cloud accepted".to_owned()
        )
    );
}

#[test]
fn local_run_state_explicitly_ignores_cloud_metadata() {
    let started = Event {
        seq: 0,
        at_ms: 1,
        kind: EventKind::RunStarted,
        data: crate::runlog::run_started("pipeline", "assignment"),
    };
    let cloud = Event {
        seq: 1,
        at_ms: 2,
        kind: EventKind::GitHubCloud,
        data: json!({"record": {"kind": "accepted"}}),
    };
    let mut state = crate::runlog::replay([started]).expect("local state");
    let before = state.clone();
    state.apply(&cloud);
    assert_eq!((state, crate::runlog::replay([cloud])), (before, None));
}
