use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;

use bureau::runlog::{Event, EventKind};
use serde_json::{Value, json};

use super::fixture::{Fixture, write};

fn metadata_file(fixture: &Fixture) -> PathBuf {
    fixture
        .record()
        .intent
        .paths
        .storage
        .session
        .join("workspace.yaml")
}

fn runtime_openings(fixture: &Fixture) -> usize {
    fixture
        .events()
        .iter()
        .filter(|event| {
            event.kind == EventKind::CopilotFactory && event.data["event"] == "runtime_opened"
        })
        .count()
}

#[derive(Debug, PartialEq, Eq)]
struct Evidence {
    trace: Vec<Value>,
    runtime_openings: usize,
    database: Vec<u8>,
    worktree: (u64, u64),
    effect: Vec<u8>,
}

fn evidence(fixture: &Fixture) -> Evidence {
    let intent = fixture.record().intent;
    let metadata = fs::metadata(&intent.workspace.directory).expect("preserved worktree");
    Evidence {
        trace: fixture.trace(),
        runtime_openings: runtime_openings(fixture),
        database: fs::read(intent.paths.storage.session.join("session.db")).expect("database"),
        worktree: (metadata.dev(), metadata.ino()),
        effect: fs::read(intent.workspace.directory.join("factory-uncommitted.txt"))
            .expect("uncommitted factory effect"),
    }
}

fn document(fixture: &Fixture) -> Value {
    let intent = fixture.record().intent;
    json!({"id": intent.session_id, "cwd": intent.workspace.directory, "summary_count": 0})
}

fn replaced(mut value: Value, field: &str, replacement: Option<Value>) -> Value {
    let fields = value.as_object_mut().expect("workspace mapping");
    if let Some(value) = replacement {
        fields.insert(field.to_owned(), value);
    } else {
        fields.remove(field);
    }
    value
}

fn yaml(value: &Value) -> String {
    serde_yaml_ng::to_string(value).expect("workspace YAML")
}

async fn paused() -> Fixture {
    let fixture = Fixture::create("pause");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert!(fixture.record().can_resume(), "{outcome:?}");
    fixture
}

fn assert_refused(
    fixture: &Fixture,
    before: &Evidence,
    events: &[Event],
    bytes: &str,
    reason: &str,
) {
    assert_eq!(&evidence(fixture), before);
    assert_eq!(
        (
            fs::read(metadata_file(fixture)).expect("unchanged YAML") == bytes.as_bytes(),
            fixture.events().starts_with(events),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (true, true, 0, 0)
    );
    assert!(
        fixture
            .record()
            .indeterminate
            .as_deref()
            .is_some_and(|error| error.contains(reason))
    );
}

async fn refuse(fixture: &Fixture, bytes: &str, reason: &str) {
    let before = evidence(fixture);
    let events = fixture.events();
    write(&metadata_file(fixture), bytes);
    fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    assert_refused(fixture, &before, &events, bytes, reason);
}

fn alternate(fixture: &Fixture, directory: bool) -> PathBuf {
    let path = fixture.root.join("other-workspace");
    if directory {
        fs::create_dir(&path).expect("different existing directory");
    } else {
        write(&path, b"not a directory");
    }
    path
}

#[tokio::test]
async fn changed_or_nondirectory_cwd_never_reinitializes_the_sdk() {
    for (directory, reason) in [(true, "cwd differs"), (false, "not a directory")] {
        let fixture = paused().await;
        let path = alternate(&fixture, directory);
        let value = replaced(document(&fixture), "cwd", Some(json!(path)));
        refuse(&fixture, &yaml(&value), reason).await;
    }
}

fn invalid_fields() -> Vec<(&'static str, Option<Value>, &'static str)> {
    vec![
        ("cwd", None, "malformed"),
        ("cwd", Some(json!(42)), "must be a string"),
        ("cwd", Some(json!("relative")), "host-absolute"),
        ("cwd", Some(json!("  ")), "host-absolute"),
        ("id", None, "malformed"),
        ("id", Some(json!("")), "session id"),
        ("id", Some(json!("another-session")), "session id"),
        ("summary_count", Some(json!("invalid")), "malformed"),
    ]
}

#[tokio::test]
async fn invalid_workspace_fields_cannot_fall_back_to_a_new_session() {
    for (field, replacement, reason) in invalid_fields() {
        let fixture = paused().await;
        let value = replaced(document(&fixture), field, replacement);
        refuse(&fixture, &yaml(&value), reason).await;
    }
}

#[tokio::test]
async fn malformed_workspace_yaml_is_preserved_without_sdk_initialization() {
    let fixture = paused().await;
    refuse(&fixture, "id: [\ncwd: 'unterminated\n", "malformed").await;
}

async fn resumes(fixture: &Fixture, value: &Value) {
    write(&metadata_file(fixture), yaml(value));
    fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume"),
            fixture.record().can_clean(),
            fixture.count(EventKind::StepFinished)
        ),
        (1, 1, true, 1),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn matching_session_and_cwd_resume_with_the_native_default_summary_count() {
    let fixture = paused().await;
    let value = replaced(document(&fixture), "summary_count", None);
    resumes(&fixture, &value).await;
}

#[tokio::test]
async fn invalid_known_optional_metadata_never_uses_a_cwd_fallback() {
    for (field, invalid) in cases::invalid() {
        let fixture = paused().await;
        let value = replaced(document(&fixture), field, Some(invalid));
        refuse(&fixture, &yaml(&value), field).await;
    }
}

#[tokio::test]
async fn valid_native_metadata_and_legacy_labels_are_not_rewritten_by_preflight() {
    let fixture = paused().await;
    let mut value = document(&fixture);
    value
        .as_object_mut()
        .expect("workspace mapping")
        .extend(cases::valid().as_object().expect("known metadata").clone());
    resumes(&fixture, &value).await;
    assert_eq!(
        fs::read_to_string(metadata_file(&fixture)).expect("workspace YAML"),
        yaml(&value)
    );
}
#[path = "workspace_identity/cases.rs"]
mod cases;
