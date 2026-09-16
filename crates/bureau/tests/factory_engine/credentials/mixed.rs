use bureau::config::Permission;
use bureau::contract::StepOutcome;
use bureau::process::Secret;
use serde_json::Value;

use super::super::fixture::Fixture;

fn authorize_forge(fixture: &mut Fixture) {
    let role = fixture.plan.roles.get_mut("worker").expect("worker");
    role.permissions
        .extend([Permission::RepoWrite, Permission::PrRead]);
}

fn fixture(mode: &str, forge: bool) -> Fixture {
    let mut fixture = Fixture::create(mode);
    let token = if mode == "same-value-credentials" {
        "offline-model-token"
    } else {
        "offline-repository-token"
    };
    fixture
        .plan
        .credentials
        .insert("unused".into(), Secret::new(token));
    if forge {
        authorize_forge(&mut fixture);
    }
    fixture
}

fn row<'a>(trace: &'a [Value], key: &str, value: &str) -> &'a Value {
    trace
        .iter()
        .find(|entry| entry[key] == value)
        .expect("expected actual trace row")
}

fn check(fixture: &Fixture, forge: bool, same_value: bool) {
    let trace = fixture.trace();
    let startup = row(&trace, "startupCredential", "original");
    let gh = row(&trace, "probeCommand", "gh");
    let expected = forge.then_some("repository");
    assert_eq!(
        (
            fixture.record().can_clean(),
            startup["startupCredential"].as_str(),
            startup["forgeCredential"].as_str(),
            gh["forgeCredential"].as_str(),
            gh["modelCarrierPresent"].as_bool(),
            gh["forgeCredentialMatchesModel"].as_bool(),
        ),
        (
            true,
            Some("original"),
            expected,
            expected,
            Some(false),
            Some(forge && same_value)
        ),
        "{trace:?}",
    );
}

async fn exercise() {
    let selected = std::env::var("BUREAU_FACTORY_MIXED_TEST").expect("explicit test mode");
    for (mode, forge) in [(selected.as_str(), true), ("model-only-credentials", false)] {
        let fixture = fixture(mode, forge);
        let outcome = fixture.engine.run(&fixture.plan).await;
        let state = bureau::runlog::replay_state(&fixture.directory()).expect("actual step result");
        assert_eq!(
            (outcome.outcome, state.steps[0].outcome),
            (StepOutcome::NoWork, Some(StepOutcome::Success)),
            "{outcome:?}",
        );
        check(&fixture, forge, selected == "same-value-credentials");
    }
}

fn isolated(mode: &str, token: &str) {
    let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "credentials::mixed::model_and_authorized_forge_credentials_remain_separate",
        ])
        .env("BUREAU_FACTORY_MIXED_TEST", mode)
        .env("GH_TOKEN", token)
        .env("COPILOT_GITHUB_TOKEN", "unapproved-ambient-model-token")
        .status()
        .expect("isolated synthetic credential environment");
    assert!(status.success());
}

#[test]
fn model_and_authorized_forge_credentials_remain_separate() {
    if std::env::var_os("BUREAU_FACTORY_MIXED_TEST").is_none() {
        isolated("mixed-credentials", "offline-repository-token");
        return isolated("same-value-credentials", "offline-model-token");
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("offline runtime")
        .block_on(exercise());
}
