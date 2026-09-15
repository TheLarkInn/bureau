use bureau::adapters::copilot_factory::types::FactoryRunStatus;
use bureau::runlog::EventKind;
use serde_json::json;

use super::fixture::{Fixture, commit, git, write};

const LSP_CONFIGURATION: &str = r#"{"lspServers":{"unapproved":{"command":"unapproved-language-server","args":["--stdio"],"fileExtensions":{".rs":"rust"}}}}"#;

fn plugin(repository: &std::path::Path, mode: &str) {
    let root = repository.join("marketplace/plugins/fixture-plugin");
    write(
        &root.join("plugin.json"),
        br#"{"name":"fixture-plugin","version":"1.0.0"}"#,
    );
    write(
        &root.join("agents/worker.agent.md"),
        "---\nname: worker\nskills: [review]\n---\nUse approved pinned context.\n",
    );
    write(
        &root.join("skills/review/SKILL.md"),
        format!(
            "---\nname: review\ndescription: Review\nuser-invocable: {}\n---\nPinned review skill.\n",
            mode != "plugin-preload"
        ),
    );
    write(&root.join(".mcp.json"),
        br#"{"mcpServers":{"bureau-io":{"type":"stdio","command":"bureau","args":["mcp","serve"]}}}"#);
    write(&repository.join("marketplace/marketplace.json"),
        br#"{"name":"local","plugins":[{"name":"fixture-plugin","source":"plugins/fixture-plugin"}]}"#);
}

fn configured(mode: &str) -> Fixture {
    let mut fixture = Fixture::create(mode);
    let repository = fixture.root.join("repository");
    plugin(&repository, mode);
    let settings = json!({"extraKnownMarketplaces": {"local": {"source": {
        "source": "directory", "path": "marketplace"}}}, "enabledPlugins": {"fixture-plugin@local": true}});
    write(
        &repository.join(".github/copilot/settings.json"),
        serde_json::to_vec(&settings).expect("settings"),
    );
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
            "offline plugin context",
        ],
    );
    fixture.plan.roles.get_mut("worker").expect("role").agent = "/fixture-plugin:worker".into();
    fixture.plan.direct_agents.clear();
    fixture
}

fn add_configuration(fixture: &Fixture, path: &str, value: &str) {
    let repository = fixture.root.join("repository");
    write(
        &repository
            .join("marketplace/plugins/fixture-plugin")
            .join(path),
        value,
    );
    commit(&repository);
}

fn runtime_opened(fixture: &Fixture) -> bool {
    fixture
        .events()
        .iter()
        .any(|event| event.data["event"] == "runtime_opened")
}

#[tokio::test]
async fn plugin_lsp_configurations_are_refused_before_runtime_initialization() {
    for path in [
        ".lsp.json",
        "lsp.json",
        ".github/lsp.json",
        "com.github.copilot/lsp.json",
    ] {
        let fixture = configured("plugin-context");
        add_configuration(&fixture, path, LSP_CONFIGURATION);
        let outcome = fixture.engine.run(&fixture.plan).await;
        assert_eq!(
            (
                runtime_opened(&fixture),
                outcome
                    .message
                    .contains("unapproved factory executable configuration")
            ),
            (false, true),
            "{path}: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn plugin_lsp_manifest_declarations_are_not_supported_execution_contexts() {
    let fixture = configured("plugin-context");
    add_configuration(&fixture, ".lsp.json", LSP_CONFIGURATION);
    add_configuration(
        &fixture,
        "plugin.json",
        r#"{"name":"fixture-plugin","version":"1.0.0","lspServers":".lsp.json"}"#,
    );
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            runtime_opened(&fixture),
            outcome.message.contains("unsupported plugin manifest")
        ),
        (false, true),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn native_plugin_catalogues_and_the_controlled_broker_match_private_pins() {
    let fixture = configured("plugin-context");
    let outcome = fixture.engine.run(&fixture.plan).await;
    let record = fixture.record();
    assert_eq!(
        (
            record.status(),
            record.intent.context.selected_agent.as_str(),
            record
                .intent
                .context
                .expected_catalog
                .bureau_io_plugins
                .contains("fixture-plugin"),
            fixture.calls("session.factory.run")
        ),
        (
            Some(FactoryRunStatus::Completed),
            "fixture-plugin:worker",
            true,
            1
        ),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn agent_preload_only_skills_need_not_appear_as_slash_commands() {
    let fixture = configured("plugin-preload");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().status(),
            fixture.calls("session.factory.run")
        ),
        (Some(FactoryRunStatus::Completed), 1),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn native_agent_skill_binding_changes_fail_before_factory_admission() {
    let fixture = configured("plugin-bad-binding");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.run"),
            fixture.count(EventKind::StepFinished)
        ),
        (0, 0)
    );
}

#[tokio::test]
async fn native_resume_restores_plugin_catalogues_without_the_original_marketplace() {
    let fixture = configured("plugin-pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    std::fs::remove_dir_all(fixture.directory().join("wt/marketplace"))
        .expect("remove original sources");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().status(),
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepStarted)
        ),
        (Some(FactoryRunStatus::Completed), 1, 1, 1)
    );
}
