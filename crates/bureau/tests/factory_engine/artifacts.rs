use bureau::contract::StepOutcome;
use bureau::process::{REDACTED, Secret};
use bureau::runlog::EventKind;

use super::fixture::Fixture;

fn artifact(fixture: &Fixture) -> String {
    let events = fixture.events();
    let finished = events
        .iter()
        .find(|event| event.kind == EventKind::StepFinished)
        .expect("finished step");
    let path = finished.data["result"]["artifacts"][0]["path"]
        .as_str()
        .expect("published artifact");
    std::fs::read_to_string(path).expect("durable artifact")
}

async fn verify_scoped_artifact() {
    let fixture = Fixture::create("credential-artifact");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let events = serde_json::to_string(&fixture.events()).expect("events");
    assert_eq!(
        (
            artifact(&fixture),
            events.contains("offline-private-credential"),
            events.contains("offline-model-token"),
            super::credentials::model_credentials(&fixture),
            fixture.trace()[0]["ambientGitHubCredential"].as_bool()
        ),
        (
            REDACTED.into(),
            false,
            false,
            vec!["original".to_owned()],
            Some(false)
        )
    );
    super::credentials::reject_missing_model_credential().await;
}

fn isolated_credentials() {
    let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "artifacts::scoped_runtime_credentials_are_scrubbed_from_artifacts",
            "--nocapture",
        ])
        .env("BUREAU_FACTORY_CREDENTIAL_TEST", "1")
        .env("GH_TOKEN", "offline-private-credential")
        .env("GITHUB_TOKEN", "offline-other-credential")
        .env("COPILOT_GITHUB_TOKEN", "offline-ambient-model-credential")
        .status()
        .expect("isolated credential test");
    assert!(status.success());
}

#[test]
fn scoped_runtime_credentials_are_scrubbed_from_artifacts() {
    if std::env::var_os("BUREAU_FACTORY_CREDENTIAL_TEST").is_none() {
        return isolated_credentials();
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("offline runtime")
        .block_on(verify_scoped_artifact());
}

#[tokio::test]
async fn public_log_redaction_never_replaces_the_original_static_factory_arguments() {
    let mut fixture = Fixture::create("private-static-argument");
    fixture
        .plan
        .credentials
        .insert("redact-only".into(), Secret::new("private-static-argument"));
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let trace = fixture.trace();
    let start = trace
        .iter()
        .find(|entry| entry["method"] == "session.factory.run")
        .expect("start");
    let public = serde_json::to_string(&fixture.events()).expect("events");
    assert_eq!(
        (
            start["params"]["args"]["mode"].as_str(),
            public.contains("private-static-argument")
        ),
        (Some("private-static-argument"), false)
    );
}

#[tokio::test]
async fn environment_templates_remain_opaque_static_factory_arguments() {
    for literal in ["$GITHUB_TOKEN", "${COPILOT_GITHUB_TOKEN}", "${HOME}"] {
        let fixture = Fixture::create(literal);
        let _outcome = fixture.engine.run(&fixture.plan).await;
        let trace = fixture.trace();
        let start = trace
            .iter()
            .find(|entry| entry["method"] == "session.factory.run")
            .expect("native factory admission");
        assert_eq!(
            (
                start["params"]["args"]["mode"].as_str(),
                fixture.record().can_clean()
            ),
            (Some(literal), true)
        );
    }
}

#[tokio::test]
async fn factory_artifacts_must_stay_inside_the_original_worktree() {
    let fixture = Fixture::create("artifact-escape");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (outcome.outcome, fixture.count(EventKind::StepFinished)),
        (StepOutcome::Failure, 1)
    );
}
