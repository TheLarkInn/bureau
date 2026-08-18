//! Offline MCP session lifetime and validation tests.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust, WorkItem};
use bureau::mcp::{BUREAU_STEP_REQUEST, BUREAU_STEP_RESULT, Paths, Session, serve};
use serde_json::json;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-mcp-session-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("create test directory");
        Self(path)
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

fn request(worktree: &Path) -> StepRequest {
    StepRequest {
        schema: SCHEMA_VERSION.to_owned(),
        run_id: "run-42".to_owned(),
        step: "implement".to_owned(),
        worktree: worktree.to_path_buf(),
        item: WorkItem::default(),
        trust: Trust::Maintainer,
        inputs: BTreeMap::from([("issue".to_owned(), json!(42))]),
        artifacts: BTreeMap::new(),
    }
}

fn result() -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Success,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: String::new(),
    }
}

#[test]
fn session_reserves_paths_environment_and_cleans_up() {
    let dir = TestDir::new("lifetime");
    let expected = request(dir.path());
    let session = Session::create(&expected).expect("create session");
    let session_dir = session.dir().to_path_buf();
    let config = session.dir().join("mcp.json");
    std::fs::write(&config, b"{}").expect("write sibling config");
    let stored = std::fs::read(session.request_path()).expect("read request");
    let parsed = StepRequest::from_json(&stored).expect("parse request");
    let created = (
        parsed == expected,
        !session.result_path().exists(),
        !session_dir.starts_with(dir.path()),
        environment_matches(&session),
        paths_share_directory(&session) && config.exists(),
    );
    assert_eq!(created, (true, true, true, true, true));
    drop(session);
    assert!(!session_dir.exists());
}

fn paths_share_directory(session: &Session) -> bool {
    session.request_path().parent() == Some(session.dir())
        && session.result_path().parent() == Some(session.dir())
}

fn environment_matches(session: &Session) -> bool {
    let request = session.request_path().to_string_lossy();
    let result = session.result_path().to_string_lossy();
    session.env().get(BUREAU_STEP_REQUEST) == Some(&request.into_owned())
        && session.env().get(BUREAU_STEP_RESULT) == Some(&result.into_owned())
}

#[test]
fn published_roundtrips_a_valid_result() {
    let dir = TestDir::new("roundtrip");
    let session = Session::create(&request(dir.path())).expect("create session");
    let before = session.published().expect("unpublished");
    let expected = result();
    std::fs::write(
        session.result_path(),
        expected.to_json().expect("serialize result"),
    )
    .expect("write result");
    let after = session.published().expect("published");
    assert_eq!((before, after), (None, Some(expected)));
}

#[test]
fn published_rejects_malformed_result() {
    let dir = TestDir::new("bad-result");
    let session = Session::create(&request(dir.path())).expect("create session");
    std::fs::write(session.result_path(), br#"{"schema":"wrong"}"#).expect("write result");
    let kind = session.published().expect_err("invalid result").kind();
    assert_eq!(kind, std::io::ErrorKind::InvalidData);
}

#[test]
fn serve_refuses_invalid_paths_before_reading() {
    let dir = TestDir::new("invalid-paths");
    let missing = Paths::new(
        dir.path().join("missing.json"),
        dir.path().join("result.json"),
    );
    let missing_kind = serve_error(missing);
    let session = Session::create(&request(dir.path())).expect("create session");
    std::fs::write(session.result_path(), b"occupied").expect("occupy result");
    let existing_kind = serve_error(session.paths());
    assert_eq!(
        (missing_kind, existing_kind),
        (
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::AlreadyExists
        )
    );
}

fn serve_error(paths: Paths) -> std::io::ErrorKind {
    serve(paths, Cursor::new(Vec::<u8>::new()), Vec::new())
        .expect_err("invalid paths")
        .kind()
}
