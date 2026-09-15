use std::fs::{self, File, FileTimes, OpenOptions};
use std::io::{self, Write as _};
use std::path::PathBuf;

use super::{Error, EventKind, Fixture, Operation, RunLog, output};

fn path(fixture: &Fixture) -> PathBuf {
    fixture.root.join("runs/writer/events.jsonl")
}

fn write(fixture: &Fixture, bytes: &[u8]) {
    fixture
        .owner
        .with_ownership(|| fs::write(path(fixture), bytes))
        .expect("fenced replacement");
}

fn append(fixture: &Fixture, bytes: &[u8]) {
    fixture
        .owner
        .with_ownership(|| {
            let mut file = OpenOptions::new().append(true).open(path(fixture))?;
            file.write_all(bytes)?;
            file.sync_all()
        })
        .expect("fenced append");
}

fn during(
    operation: Operation,
    mut fixture: Fixture,
    mut change: impl FnMut(&mut Fixture, usize),
) -> (Result<bool, Error>, usize) {
    let root = fixture.root.clone();
    let mut attempts = 0;
    let result = operation.run(&root, || {
        attempts += 1;
        change(&mut fixture, attempts);
    });
    fixture.close();
    (result, attempts)
}

fn replace_directory(fixture: &Fixture) -> io::Result<()> {
    let directory = fixture.root.join("runs/writer");
    let bytes = fs::read(directory.join("events.jsonl"))?;
    fs::rename(&directory, fixture.root.join("previous-writer"))?;
    fs::create_dir(&directory)?;
    fs::write(directory.join("events.jsonl"), bytes)
}

fn replace_file(fixture: &Fixture) -> io::Result<()> {
    let file = path(fixture);
    let bytes = fs::read(&file)?;
    fs::rename(&file, fixture.root.join("previous-events"))?;
    fs::write(file, bytes)
}

fn replace_root(fixture: &Fixture) -> io::Result<()> {
    let runs = fixture.root.join("runs");
    let previous = fixture.root.join("previous-runs");
    fs::rename(&runs, &previous)?;
    fs::create_dir(&runs)?;
    fs::rename(previous.join("writer"), runs.join("writer"))
}

#[test]
fn exact_bytes_and_replacement_identities_are_revalidated() {
    for replace in [replace_directory, replace_file, replace_root] {
        let (result, attempts) = during(Operation::Claim, Fixture::new(), |fixture, attempt| {
            if attempt == 1 {
                fixture
                    .owner
                    .with_ownership(|| replace(fixture))
                    .expect("replace authority");
            }
        });
        assert_eq!((result.expect("current authority"), attempts), (true, 2));
    }
}

fn corrupt_same_length(fixture: &Fixture) -> io::Result<()> {
    let file = path(fixture);
    let modified = fs::metadata(&file)?.modified()?;
    let mut bytes = fs::read(&file)?;
    bytes[0] = b'!';
    fs::write(&file, bytes)?;
    File::open(file)?.set_times(FileTimes::new().set_modified(modified))
}

#[test]
fn restored_mtime_and_unchanged_size_do_not_authorize_changed_bytes() {
    let (result, attempts) = during(Operation::Claim, Fixture::new(), |fixture, attempt| {
        if attempt == 1 {
            fixture
                .owner
                .with_ownership(|| corrupt_same_length(fixture))
                .expect("corrupt log");
        }
    });
    assert_eq!((result.is_err(), attempts), (true, 2));
}

#[test]
fn added_unknown_directory_invalidates_prepared_membership() {
    let (result, attempts) = during(Operation::Claim, Fixture::new(), |fixture, attempt| {
        if attempt == 1 {
            fs::create_dir(fixture.root.join("runs/unknown")).expect("unknown history");
        }
    });
    assert_eq!((result.is_err(), attempts), (true, 2));
}

#[test]
fn stable_framed_corruption_is_not_ignored_as_output_or_a_torn_tail() {
    for operation in [Operation::Observe, Operation::Claim] {
        let (result, attempts) = during(operation, Fixture::new(), |fixture, attempt| {
            if attempt == 1 {
                append(fixture, b"invalid event\n");
            }
        });
        assert_eq!((result.is_err(), attempts), (true, 2));
    }
}

#[test]
fn appended_factory_authority_is_replayed_before_admission() {
    let (result, attempts) = during(Operation::Claim, Fixture::new(), |fixture, attempt| {
        if attempt == 1 {
            fixture
                .owner
                .with_ownership(|| {
                    fixture
                        .log
                        .append(EventKind::CopilotFactory, serde_json::json!({}))
                })
                .expect("new factory event");
        }
    });
    assert_eq!((result.is_err(), attempts), (true, 2));
}

#[test]
fn stale_parse_errors_are_replaced_by_revalidated_authority() {
    let fixture = Fixture::new();
    let header = fs::read(path(&fixture)).expect("valid header");
    write(&fixture, b"invalid event\n");
    let (result, attempts) = during(Operation::Claim, fixture, |fixture, attempt| {
        if attempt == 1 {
            write(fixture, &header);
        }
    });
    assert_eq!((result.expect("repaired authority"), attempts), (true, 2));
}

#[test]
fn completing_a_torn_tail_replays_from_the_verified_event_boundary() {
    let fixture = Fixture::new();
    append(&fixture, b"{\"seq\":");
    let (result, attempts) = during(Operation::Claim, fixture, |fixture, attempt| {
        if attempt == 1 {
            fixture
                .owner
                .with_ownership(|| {
                    let mut log = RunLog::resume(path(fixture).parent().expect("run"), &[])?;
                    log.append(EventKind::Output, output(None, "combined", "completed"))
                })
                .expect("complete repaired tail");
        }
    });
    assert_eq!((result.expect("repaired tail"), attempts), (true, 2));
}

#[test]
fn live_owner_first_record_exemption_is_rechecked_after_preparation() {
    let fixture = Fixture::new();
    write(&fixture, b"{\"seq\":");
    let (result, attempts) = during(Operation::Claim, fixture, |fixture, attempt| {
        if attempt == 1 {
            fixture.owner.release().expect("release first-record owner");
        }
    });
    assert_eq!((result.is_err(), attempts), (true, 1));
}

#[test]
fn first_record_publication_invalidates_an_unpublished_snapshot() {
    let fixture = Fixture::new();
    let header = fs::read(path(&fixture)).expect("valid header");
    write(&fixture, b"{\"seq\":");
    let (result, attempts) = during(Operation::Claim, fixture, |fixture, attempt| {
        if attempt == 1 {
            write(fixture, &header);
        }
    });
    assert_eq!(
        (result.expect("published first record"), attempts),
        (true, 2)
    );
}

#[test]
fn output_after_cloud_only_history_is_not_a_pipeline_noop() {
    let fixture = Fixture::new();
    write(
        &fixture,
        b"{\"seq\":0,\"at_ms\":0,\"kind\":\"github_cloud\",\"data\":{}}\n",
    );
    let (result, attempts) = during(Operation::Claim, fixture, |fixture, attempt| {
        if attempt == 1 {
            fixture
                .owner
                .with_ownership(|| {
                    fixture.log.append(
                        EventKind::Output,
                        output(None, "combined", "not a pipeline"),
                    )
                })
                .expect("output after cloud history");
        }
    });
    assert_eq!((result.is_err(), attempts), (true, 2));
}
