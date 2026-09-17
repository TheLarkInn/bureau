//! A torn append lacks a newline; malformed complete records are durable evidence.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use bureau::runlog::{self, EventKind, RunLog, output, run_started};
use bureau::state::{LeaseOwner, Store};

#[path = "edge/testdir.rs"]
mod testdir;

use testdir::TestDir;

fn log(root: &Path) -> std::path::PathBuf {
    let mut log = RunLog::create(&root.join("runs"), "run", &[]).expect("log");
    log.append(EventKind::RunStarted, run_started("run", "work"))
        .expect("run start");
    let directory = log.dir().to_path_buf();
    log.close().expect("close");
    directory
}

fn write_tail(directory: &Path, tail: &str) -> String {
    let path = directory.join(runlog::EVENTS_FILE);
    let mut bytes = fs::read_to_string(&path).expect("prefix");
    bytes.push_str(tail);
    fs::write(path, &bytes).expect("inject tail");
    bytes
}

fn errors(directory: &Path) -> [Option<io::ErrorKind>; 4] {
    [
        runlog::read_events_tolerant(directory)
            .err()
            .map(|error| error.kind()),
        runlog::replay_state(directory)
            .err()
            .map(|error| error.kind()),
        RunLog::resume(directory, &[])
            .err()
            .map(|error| error.kind()),
        runlog::read_events(directory)
            .err()
            .map(|error| error.kind()),
    ]
}

fn check_corruption(tail: &str) {
    let root = TestDir::new("framed-corruption");
    let directory = log(root.path());
    let bytes = write_tail(&directory, tail);
    let rejected = errors(&directory);
    assert_eq!(
        (
            rejected,
            fs::read_to_string(directory.join(runlog::EVENTS_FILE)).expect("preserved bytes")
        ),
        ([Some(io::ErrorKind::InvalidData); 4], bytes),
        "framed corruption must remain intact: {tail:?}"
    );
}

#[test]
fn complete_corruption_is_rejected_without_erasing_any_evidence() {
    for tail in [
        "{not-json}\n",
        "{\"seq\":\"invalid\",\"at_ms\":0,\"kind\":\"output\",\"data\":{}}\n",
        "{\"seq\":1,\"at_ms\":0,\"kind\":\"unknown\",\"data\":{}}\n",
        "{not-json}\n  \n\n",
        "{not-json}\n{\"seq\":",
        "{not-json}\r\n",
    ] {
        check_corruption(tail);
    }
}

#[test]
fn a_complete_final_json_record_without_newline_is_preserved_on_resume() {
    let root = TestDir::new("complete-no-newline");
    let directory = log(root.path());
    let record = "{\"seq\":1,\"at_ms\":0,\"kind\":\"output\",\"data\":{}}";
    let bytes = write_tail(&directory, record);
    let events = runlog::read_events_tolerant(&directory).expect("complete JSON");
    let mut resumed = RunLog::resume(&directory, &[]).expect("resume");
    let sequence = resumed
        .append(EventKind::Output, output(None, "stdout", "next"))
        .expect("separate append");
    resumed.close().expect("close");
    assert_eq!(
        (
            events.len(),
            sequence,
            runlog::read_events(&directory)
                .expect("resumed events")
                .len(),
            fs::read_to_string(directory.join(runlog::EVENTS_FILE))
                .expect("bytes")
                .starts_with(&format!("{bytes}\n"))
        ),
        (2, 2, 3, true)
    );
}

#[test]
fn read_only_partial_tail_does_not_repair_until_resume() {
    let root = TestDir::new("partial-prefix");
    let directory = log(root.path());
    let bytes = write_tail(&directory, "{\"seq\":");
    let events = runlog::read_events_tolerant(&directory).expect("valid prefix");
    let unchanged = fs::read_to_string(directory.join(runlog::EVENTS_FILE)).expect("bytes");
    RunLog::resume(&directory, &[])
        .expect("repair tail")
        .close()
        .expect("close");
    assert_eq!(
        (
            events.len(),
            unchanged == bytes,
            fs::read_to_string(directory.join(runlog::EVENTS_FILE))
                .expect("repaired bytes")
                .ends_with('\n')
        ),
        (1, true, true)
    );
}

#[test]
fn reading_a_corrupt_record_cannot_make_fresh_admission_succeed() {
    let root = TestDir::new("admission-corruption");
    let directory = log(root.path());
    let bytes = write_tail(&directory, "{not-json}\n");
    let _rejected = runlog::read_events(&directory);
    let store = Arc::new(Store::open(&root.path().join("state.db")).expect("store"));
    let owner = LeaseOwner::new(store, "work", "github", "1", "new").expect("owner");
    let result = owner.claim_fresh(
        std::time::Duration::from_secs(60),
        &root.path().join("runs"),
    );
    assert_eq!(
        (
            result.is_err(),
            owner.owns().expect("unclaimed"),
            fs::read_to_string(directory.join(runlog::EVENTS_FILE)).expect("evidence")
        ),
        (true, false, bytes)
    );
}
