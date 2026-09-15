#[path = "credentials/containment.rs"]
mod containment;
#[path = "credentials/masking.rs"]
mod masking;
#[path = "credentials/mixed.rs"]
mod mixed;

use bureau::process::Secret;
use bureau::runlog::EventKind;

use super::fixture::Fixture;

pub async fn reject_missing_model_credential() {
    let mut fixture = Fixture::create("success");
    fixture.plan.credentials.remove("copilot-model");
    fixture
        .plan
        .credentials
        .insert("unused".into(), Secret::new("offline-repository-token"));
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.count(EventKind::CopilotFactory),
            fixture.count(EventKind::StepFinished),
            outcome.message.contains("unavailable"),
            fixture.directory().join("PAUSE").exists()
        ),
        (0, 0, true, true),
        "{outcome:?}"
    );
}

fn requested_reference(fixture: &mut Fixture, reference: &str) {
    fixture.plan.pipeline.steps[0]
        .copilot_factory
        .as_mut()
        .expect("factory")
        .model_credential = reference.into();
}

pub fn model_credentials(fixture: &Fixture) -> Vec<String> {
    fixture
        .trace()
        .iter()
        .filter_map(|entry| entry["modelCredential"].as_str().map(str::to_owned))
        .collect()
}

fn startup_credentials(fixture: &Fixture) -> Vec<String> {
    fixture
        .trace()
        .iter()
        .filter_map(|entry| entry["startupCredential"].as_str().map(str::to_owned))
        .collect()
}

fn matching_credentials(fixture: &Fixture) {
    assert_eq!(startup_credentials(fixture), model_credentials(fixture));
}

#[tokio::test]
async fn model_authentication_never_falls_back_to_a_repository_credential() {
    reject_missing_model_credential().await;
}

#[tokio::test]
async fn an_empty_model_credential_never_launches_the_runtime() {
    let mut fixture = Fixture::create("success");
    fixture
        .plan
        .credentials
        .insert("copilot-model".into(), Secret::new(""));
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.count(EventKind::CopilotFactory),
            fixture.count(EventKind::StepFinished),
            outcome.message.contains("empty value")
        ),
        (0, 0, true),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn explicit_session_credentials_do_not_bypass_factory_eligibility() {
    let fixture = Fixture::create("ineligible-session");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.listRuns"),
            fixture.calls("session.factory.run"),
            fixture.count(EventKind::StepFinished),
            outcome.message.contains("agent_factories_unavailable")
        ),
        (1, 0, 0, true),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn cold_recovery_requires_the_original_model_credential() {
    let mut fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    fixture.plan.credentials.remove("copilot-model");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    matching_credentials(&fixture);
    assert_eq!(
        (
            fixture.calls("session.resume"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepStarted),
            fixture.count(EventKind::StepFinished),
            fixture.directory().join("wt").exists()
        ),
        (0, 0, 1, 0, true)
    );
}

#[tokio::test]
async fn cold_recovery_ignores_a_replacement_live_model_credential_reference() {
    let mut fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    requested_reference(&mut fixture, "replacement");
    fixture
        .plan
        .credentials
        .insert("replacement".into(), Secret::new("unapproved-token"));
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().intent.factory.model_credential.as_str(),
            fixture.calls("session.factory.resume"),
            model_credentials(&fixture)
        ),
        ("copilot-model", 1, vec!["original".to_owned(); 3])
    );
}

#[tokio::test]
async fn cold_recovery_resolves_the_same_reference_to_its_current_secret() {
    let mut fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    fixture.plan.credentials.insert(
        "copilot-model".into(),
        Secret::new("offline-rotated-model-token"),
    );
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    matching_credentials(&fixture);
    assert_eq!(
        (
            fixture.calls("session.factory.resume"),
            model_credentials(&fixture)
        ),
        (
            1,
            vec![
                "original".to_owned(),
                "rotated".to_owned(),
                "rotated".to_owned()
            ]
        )
    );
}

#[tokio::test]
async fn model_credentials_are_not_disclosed_by_startup_or_rpc_errors() {
    for mode in ["startup-auth-rejected", "credential-rpc-error"] {
        let fixture = Fixture::create(mode);
        let outcome = fixture.engine.run(&fixture.plan).await;
        let log = serde_json::to_string(&fixture.events()).expect("public log");
        let pause = std::fs::read_to_string(fixture.directory().join("PAUSE")).expect("pause");
        assert_eq!(
            (
                fixture.count(EventKind::StepFinished),
                format!("{outcome:?}{log}{pause}").contains("offline-model-token")
            ),
            (0, false),
            "{mode}"
        );
    }
}

#[tokio::test]
async fn model_credentials_are_not_granted_to_extension_environment_requests() {
    let fixture = Fixture::create("credential-permission");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().can_clean(),
            fixture.calls("session.permissions.handlePendingPermissionRequest")
        ),
        (true, 2)
    );
}

async fn rejected_broker_expansion() {
    let fixture = Fixture::create("success");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture
                .events()
                .iter()
                .any(|event| event.data["event"] == "runtime_opened"),
            outcome.message.contains("environment expansion")
        ),
        (false, true),
        "{outcome:?}"
    );
}

fn isolated_broker_expansion(name: &str) {
    let fixture = Fixture::create("success");
    let temporary = fixture.root.join(name);
    std::fs::create_dir(&temporary).expect("literal expansion path");
    let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "credentials::broker_environment_expansion_is_rejected_before_runtime_execution",
        ])
        .env("BUREAU_FACTORY_BROKER_TEST", "1")
        .env("TMPDIR", temporary)
        .status()
        .expect("isolated broker environment");
    assert!(status.success());
}

fn isolated_broker_roots() {
    for name in ["${GITHUB_TOKEN}", "${COPILOT_GITHUB_TOKEN}", "$HOME"] {
        isolated_broker_expansion(name);
    }
}

#[test]
fn broker_environment_expansion_is_rejected_before_runtime_execution() {
    if std::env::var_os("BUREAU_FACTORY_BROKER_TEST").is_none() {
        return isolated_broker_roots();
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("offline runtime")
        .block_on(rejected_broker_expansion());
}
