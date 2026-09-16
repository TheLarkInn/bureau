use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use bureau::runlog::EventKind;
use serde_json::Value;

use super::super::fixture::{Fixture, write};

enum Change {
    Settings(&'static str),
    Legacy(&'static str),
    MissingSettings,
    NonFileSettings,
}

const CASES: &[(&str, Change, &str)] = &[
    ("missing settings", Change::MissingSettings, "auth policy"),
    ("malformed settings", Change::Settings("{"), "auth policy"),
    (
        "omitted git",
        Change::Settings(r#"{"sandbox":{"auth":{"gh":false}}}"#),
        "auth policy",
    ),
    (
        "omitted gh",
        Change::Settings(r#"{"sandbox":{"auth":{"git":false}}}"#),
        "auth policy",
    ),
    (
        "null auth",
        Change::Settings(r#"{"sandbox":{"auth":null}}"#),
        "auth policy",
    ),
    (
        "null git",
        Change::Settings(r#"{"sandbox":{"auth":{"git":null,"gh":false}}}"#),
        "auth policy",
    ),
    (
        "null gh",
        Change::Settings(r#"{"sandbox":{"auth":{"git":false,"gh":null}}}"#),
        "auth policy",
    ),
    (
        "unreadable settings document",
        Change::NonFileSettings,
        "not a plain file",
    ),
    (
        "empty legacy sandbox mask",
        Change::Legacy(r#"{"sandbox":{}}"#),
        "cannot override factory policy: sandbox",
    ),
    (
        "null legacy sandbox mask",
        Change::Legacy(r#"{"sandbox":null}"#),
        "cannot override factory policy: sandbox",
    ),
    (
        "malformed legacy config",
        Change::Legacy("{"),
        "invalid private native config",
    ),
    (
        "nonobject legacy config",
        Change::Legacy("null"),
        "must be an object",
    ),
];

#[derive(Debug, PartialEq, Eq)]
enum Contents {
    Missing,
    Directory,
    File(Vec<u8>),
}

fn contents(path: &Path) -> Contents {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Contents::Missing,
        Err(error) => panic!("read policy metadata {}: {error}", path.display()),
    };
    if metadata.is_dir() {
        return Contents::Directory;
    }
    Contents::File(fs::read(path).expect("policy bytes"))
}

#[derive(Debug, PartialEq)]
struct Evidence {
    trace: Vec<Value>,
    runtime_openings: usize,
    documents: [Contents; 2],
}

fn evidence(fixture: &Fixture) -> Evidence {
    let home = fixture.record().intent.paths.storage.copilot_home;
    Evidence {
        trace: fixture.trace(),
        runtime_openings: fixture
            .events()
            .iter()
            .filter(|event| {
                event.kind == EventKind::CopilotFactory && event.data["event"] == "runtime_opened"
            })
            .count(),
        documents: ["settings.json", "config.json"].map(|file| contents(&home.join(file))),
    }
}

fn change(fixture: &Fixture, update: &Change) {
    let home = fixture.record().intent.paths.storage.copilot_home;
    let settings = home.join("settings.json");
    fs::set_permissions(&settings, fs::Permissions::from_mode(0o600)).expect("owned policy");
    match update {
        Change::Settings(value) => write(&settings, value),
        Change::Legacy(value) => write(&home.join("config.json"), value),
        Change::MissingSettings => fs::remove_file(settings).expect("missing policy"),
        Change::NonFileSettings => {
            fs::remove_file(&settings).expect("remove policy file");
            fs::create_dir(settings).expect("unreadable JSON document");
        }
    }
}

fn check(fixture: &Fixture, before: &Evidence, reason: &str) {
    assert_eq!(
        &evidence(fixture),
        before,
        "policy and native initialization must be untouched"
    );
    assert_eq!(
        (
            fixture.calls("session.resume"),
            fixture.calls("session.factory.resume"),
            fixture.calls("session.factory.run"),
            fixture.count(EventKind::StepFinished),
            fixture.directory().join("PAUSE").is_file(),
            fixture
                .record()
                .indeterminate
                .as_deref()
                .is_some_and(|error| error.contains(reason))
        ),
        (0, 0, 1, 0, true, true),
        "{reason}"
    );
}

async fn refuse(name: &str, update: &Change, reason: &str) {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    change(&fixture, update);
    let before = evidence(&fixture);
    fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit recovery");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert!(!fixture.record().can_clean(), "{name}: {outcome:?}");
    check(&fixture, &before, reason);
}

#[tokio::test]
async fn invalid_or_masking_auth_documents_never_reinitialize_or_rewrite() {
    for (name, update, reason) in CASES {
        refuse(name, update, reason).await;
    }
}

#[tokio::test]
async fn unrelated_legacy_metadata_does_not_mask_private_auth_policy() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let metadata = r#"{"last_version":"offline-fixture-c"}"#;
    change(&fixture, &Change::Legacy(metadata));
    fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit recovery");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().can_clean(),
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume"),
            &evidence(&fixture).documents[1]
        ),
        (true, 1, 1, &Contents::File(metadata.as_bytes().to_vec())),
        "{outcome:?}"
    );
}
