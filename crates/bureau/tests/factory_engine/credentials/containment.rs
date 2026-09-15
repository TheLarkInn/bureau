use bureau::config::Permission;
use bureau::runlog::EventKind;
use std::os::unix::fs::PermissionsExt as _;

use super::super::fixture::{Fixture, git, write};

fn commit_input(fixture: &Fixture, path: &str, value: &str) {
    let repository = fixture.root.join("repository");
    write(&repository.join(path), value);
    git(&repository, &["add", "-A"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "offline unapproved executable configuration",
        ],
    );
}

#[tokio::test]
async fn unapproved_hooks_and_live_token_mcp_arguments_fail_before_runtime_execution() {
    for (path, value) in [
        (
            ".github/hooks/session.json",
            r#"{"hooks":{"sessionStart":[]}}"#,
        ),
        (
            ".mcp.json",
            r#"{"mcpServers":{"unapproved":{"command":"echo","args":["$GITHUB_TOKEN"]}}}"#,
        ),
    ] {
        let fixture = Fixture::create("success");
        commit_input(&fixture, path, value);
        let outcome = fixture.engine.run(&fixture.plan).await;
        assert_eq!(
            (
                fixture.count(EventKind::CopilotFactory),
                outcome.message.contains("hooks/MCP")
            ),
            (0, true),
            "{path}: {outcome:?}"
        );
    }
}

fn descendants(fixture: &Fixture) -> Vec<String> {
    let mut descendants: Vec<String> = fixture
        .trace()
        .iter()
        .filter_map(|entry| entry["authDescendant"].as_str().map(str::to_owned))
        .collect();
    descendants.sort();
    descendants
}

#[tokio::test]
async fn scoped_model_auth_does_not_authorize_descendant_environments_or_forge_tools() {
    let mut fixture = Fixture::create("success");
    let permissions = vec![Permission::ModelInvoke, Permission::RepoWrite];
    fixture
        .plan
        .roles
        .get_mut("worker")
        .expect("role")
        .permissions = permissions.clone();
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().can_clean(),
            descendants(&fixture),
            fixture.plan.roles["worker"].permissions.clone()
        ),
        (
            true,
            vec!["extension".to_owned(), "mcp".to_owned(), "shell".to_owned()],
            permissions
        ),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn declaring_model_auth_does_not_grant_model_invoke() {
    let mut fixture = Fixture::create("success");
    fixture
        .plan
        .roles
        .get_mut("worker")
        .expect("role")
        .permissions
        .clear();
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.count(EventKind::CopilotFactory),
            outcome.message.contains("model:invoke"),
            fixture.count(EventKind::StepFinished)
        ),
        (0, true, 0),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn project_lsp_is_refused_before_a_runtime_receives_model_auth() {
    let fixture = Fixture::create("success");
    commit_input(
        &fixture,
        ".github/lsp.json",
        r#"{"lspServers":{"unapproved":{"command":"echo"}}}"#,
    );
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture
                .events()
                .iter()
                .any(|event| event.data["event"] == "runtime_opened"),
            outcome
                .message
                .contains("unapproved factory executable configuration")
        ),
        (false, true),
        "{outcome:?}"
    );
}

fn change_policy(fixture: &Fixture, file: &str) {
    let home = fixture.record().intent.paths.storage.copilot_home;
    let settings = home.join("settings.json");
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o600))
        .expect("owned policy mutation");
    match file {
        "missing" => std::fs::remove_file(settings).expect("missing policy"),
        "settings.json" => write(&settings, "{}"),
        "config.json" => write(
            &home.join(file),
            r#"{"sandbox":{"auth":{"git":true,"gh":true}}}"#,
        ),
        "lsp.json" => write(&home.join(file), r#"{"lspServers":{}}"#),
        _ => panic!("unsupported policy fixture"),
    }
}

async fn rejected_policy_change(file: &str) {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    change_policy(&fixture, file);
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit recovery attempt");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.resume"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepFinished),
            fixture.directory().join("PAUSE").exists()
        ),
        (0, 0, 0, true),
        "{file}: {outcome:?}"
    );
}

#[tokio::test]
async fn cold_recovery_refuses_missing_or_overridden_auth_policy() {
    for file in ["missing", "settings.json", "config.json", "lsp.json"] {
        rejected_policy_change(file).await;
    }
}

#[tokio::test]
async fn repository_settings_cannot_authorize_model_identity_for_shell_commands() {
    let mut fixture = Fixture::create("success");
    commit_input(
        &fixture,
        ".github/copilot/settings.json",
        r#"{"sandbox":{"auth":{"git":true,"gh":true}}}"#,
    );
    fixture
        .plan
        .roles
        .get_mut("worker")
        .expect("role")
        .permissions
        .push(Permission::RepoWrite);
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (fixture.record().can_clean(), descendants(&fixture).len()),
        (true, 3),
        "{outcome:?}"
    );
}

async fn without_model_alias() {
    let mut fixture = Fixture::create("aliased-credentials");
    fixture
        .plan
        .roles
        .get_mut("worker")
        .expect("role")
        .permissions = vec![Permission::ModelInvoke, Permission::RepoWrite];
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert!(fixture.record().can_clean(), "{outcome:?}");
    assert_eq!(descendants(&fixture).len(), 3);
}

fn isolated_model_alias() {
    let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "credentials::containment::model_credentials_do_not_create_an_unauthorized_forge_channel",
        ])
        .env("BUREAU_FACTORY_ALIAS_TEST", "1")
        .env("GH_TOKEN", "offline-model-token")
        .status()
        .expect("isolated model alias");
    assert!(status.success());
}

#[test]
fn model_credentials_do_not_create_an_unauthorized_forge_channel() {
    if std::env::var_os("BUREAU_FACTORY_ALIAS_TEST").is_none() {
        return isolated_model_alias();
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("offline runtime")
        .block_on(without_model_alias());
}
