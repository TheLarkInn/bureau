//! Layer 1 fake-adapter tests (DESIGN.md section 7): the transcript
//! fixture format, schema versioning, and `record` capture through the
//! layer-0 contract.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::adapters::fake::{Chunk, FakeError, Stream, Transcript};
use bureau::contract::SCHEMA_VERSION;
use bureau::process::{SpawnOutcome, SpawnResult};

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

fn sample() -> Transcript {
    Transcript {
        schema: SCHEMA_VERSION.to_owned(),
        chunks: vec![
            Chunk {
                delay_ms: 0,
                stream: Stream::Stdout,
                data: "hello\n".to_owned(),
            },
            Chunk {
                delay_ms: 5,
                stream: Stream::Stderr,
                data: "oops\n".to_owned(),
            },
        ],
        exit_code: 7,
        usage: bureau::adapters::Usage::unknown("fake"),
    }
}

#[test]
fn transcript_round_trips_through_a_fixture_file() {
    let dir = TestDir::new("roundtrip");
    let path = dir.path().join("fixture.json");
    sample().save(&path).expect("save");
    assert_eq!(Transcript::load(&path).expect("load"), sample());
}

#[test]
fn wrong_schema_is_rejected_with_the_received_value() {
    let dir = TestDir::new("schema");
    let path = dir.path().join("fixture.json");
    std::fs::write(&path, r#"{"schema":"v9","chunks":[],"exit_code":0}"#).expect("write");
    let error = Transcript::load(&path).expect_err("must fail");
    match error {
        FakeError::Schema { received } => assert_eq!(received, "v9"),
        other => panic!("expected schema error, got {other:?}"),
    }
}

#[test]
fn record_builds_a_replayable_transcript() {
    let result = SpawnResult {
        outcome: SpawnOutcome::Exited,
        exit_code: Some(3),
        stdout: b"hello\n".to_vec(),
        stderr: b"oops\n".to_vec(),
        duration: std::time::Duration::from_millis(1),
        error: None,
    };
    let transcript = Transcript::from_result(&result);
    assert_eq!(transcript.exit_code, 3);
    assert_eq!(transcript.chunks.len(), 2);
    assert!(transcript.chunks.iter().all(|c| c.delay_ms == 0));
}

#[test]
fn record_collapses_a_failed_spawn_to_exit_1() {
    let result = SpawnResult {
        outcome: SpawnOutcome::SpawnFailed,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        duration: std::time::Duration::from_millis(1),
        error: Some("nope".to_owned()),
    };
    let transcript = Transcript::from_result(&result);
    assert_eq!(transcript.exit_code, 1);
    assert!(transcript.chunks.is_empty());
}
