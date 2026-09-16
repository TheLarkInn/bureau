use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt as _, symlink};
use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use super::publish_new;
use crate::Error;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn outcome(result: Result<(), Error>) -> Result<(), io::ErrorKind> {
    result.map_err(|error| match error {
        Error::Io { source, .. } => source.kind(),
        other => panic!("unexpected publication error: {other}"),
    })
}

fn identity(path: &Path) -> (u64, u32) {
    let metadata = fs::symlink_metadata(path).expect("snapshot identity");
    (metadata.ino(), metadata.mode())
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bureau-atomic-publication-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("fixture");
        Self { root }
    }

    fn stage(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir(&path).expect("stage");
        fs::write(path.join("payload"), name).expect("payload");
        path
    }

    fn destination(&self) -> PathBuf {
        self.root.join("snapshot")
    }

    fn publish(&self, stage: &Path) -> Result<(), io::ErrorKind> {
        outcome(publish_new(&self.root, stage, &self.destination()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove owned fixture");
    }
}

fn existing(fixture: &Fixture, kind: &str) {
    let destination = fixture.destination();
    match kind {
        "empty" => fs::create_dir(destination).expect("empty destination"),
        "populated" => {
            fs::create_dir(&destination).expect("populated destination");
            fs::write(destination.join("retained"), "original").expect("retained bytes");
        }
        "file" => fs::write(destination, "original").expect("existing file"),
        "symlink" => symlink("absent", destination).expect("existing symlink"),
        _ => panic!("unknown destination fixture"),
    }
}

fn competing(fixture: &Fixture, stage: &Path, barrier: &Barrier) -> Result<(), io::ErrorKind> {
    barrier.wait();
    fixture.publish(stage)
}

fn race(fixture: &Fixture, stages: &[PathBuf; 2]) -> [Result<(), io::ErrorKind>; 2] {
    let barrier = Barrier::new(2);
    thread::scope(|scope| {
        let first = scope.spawn(|| competing(fixture, &stages[0], &barrier));
        let second = scope.spawn(|| competing(fixture, &stages[1], &barrier));
        [
            first.join().expect("first publisher"),
            second.join().expect("second publisher"),
        ]
    })
}

#[test]
fn absent_destination_is_published_as_one_complete_directory() {
    let fixture = Fixture::new();
    let stage = fixture.stage("complete");
    let original = identity(&stage);
    let result = fixture.publish(&stage);
    assert_eq!(
        (
            result,
            stage.exists(),
            identity(&fixture.destination()),
            fs::read(fixture.destination().join("payload")).expect("published bytes"),
        ),
        (Ok(()), false, original, b"complete".to_vec())
    );
}

#[test]
fn no_replace_preserves_every_existing_destination_kind() {
    for kind in ["empty", "populated", "file", "symlink"] {
        let fixture = Fixture::new();
        let stage = fixture.stage("candidate");
        existing(&fixture, kind);
        let before = identity(&fixture.destination());
        assert_eq!(
            (
                fixture.publish(&stage),
                identity(&fixture.destination()),
                fs::read(stage.join("payload")).expect("unpublished bytes"),
            ),
            (
                Err(io::ErrorKind::AlreadyExists),
                before,
                b"candidate".to_vec()
            ),
            "{kind}"
        );
    }
}

#[test]
fn concurrent_publishers_cannot_replace_the_winning_directory() {
    let fixture = Fixture::new();
    let stages = [fixture.stage("first"), fixture.stage("second")];
    let before = stages.each_ref().map(|stage| identity(stage));
    let outcomes = race(&fixture, &stages);
    let winner = outcomes.iter().position(Result::is_ok).expect("one winner");
    let loser = 1 - winner;
    assert_eq!(
        (
            outcomes[loser],
            stages[winner].exists(),
            stages[loser].exists(),
            identity(&fixture.destination()),
        ),
        (
            Err(io::ErrorKind::AlreadyExists),
            false,
            true,
            before[winner]
        )
    );
}

#[test]
fn missing_stage_retains_the_publication_error_and_destination() {
    let fixture = Fixture::new();
    let destination = fixture.destination();
    let error = publish_new(&fixture.root, &fixture.root.join("missing"), &destination)
        .expect_err("missing stage");
    let Error::Io {
        operation,
        path,
        source,
    } = error
    else {
        panic!("expected publication I/O error");
    };
    assert_eq!(
        (operation, path, source.kind(), destination.exists()),
        (
            "publish new code snapshot",
            destination,
            io::ErrorKind::NotFound,
            false
        )
    );
}
