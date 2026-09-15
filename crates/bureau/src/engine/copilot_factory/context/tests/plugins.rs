use std::fs;

use serde_json::json;

use super::support::{Fixture, plan, role, settings, write};

fn change_original(path: &std::path::Path, remove: bool) {
    if remove {
        fs::remove_dir_all(path).expect("remove original");
    } else {
        write(&path.join("agents/worker.agent.md"), b"Changed prompt.\n");
    }
}

fn original_changed(remove: bool) -> (bool, bool, bool) {
    let fixture = Fixture::new();
    let role = role("/review:worker");
    let mut plan = plan(&role);
    let original = fixture.plugin("review");
    fixture.set_settings(&settings(json!({"review@local": true})));
    let source = fixture.pinned_role(&mut plan, &role);
    let settings = fs::read(fixture.settings_path()).expect("settings");
    change_original(&original, remove);
    let pins = fixture
        .prepare(&plan, &role)
        .expect("existing role snapshot");
    (
        pins.plugins[0].source == source && pins.selected_agent == "review:worker",
        fs::read(pins.plugins[0].directory.join("agents/worker.agent.md"))
            .expect("private agent")
            .ends_with(b"Pinned agent.\n"),
        fs::read(fixture.settings_path()).expect("unchanged settings") == settings,
    )
}

#[test]
fn role_uses_run_snapshot_when_original_changes_or_disappears() {
    for remove in [false, true] {
        assert_eq!(original_changed(remove), (true, true, true));
    }
}

#[test]
fn local_enabled_plugins_are_pinned_without_activating_disabled_plugins() {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    let original = fixture.plugin("review");
    fixture.set_settings(&settings(
        json!({"review@local": true, "disabled@local": false}),
    ));
    let pins = fixture
        .prepare(&plan(&role), &role)
        .expect("enabled local plugin");
    assert_eq!(
        (
            pins.plugins.len(),
            pins.expected_catalog.agents.len(),
            pins.expected_catalog.skills.contains_key("review:review")
        ),
        (1, 2, true)
    );
    assert_eq!(
        fs::read(pins.plugins[0].directory.join("scripts/check.sh")).expect("pinned script"),
        fs::read(original.join("scripts/check.sh")).expect("source script")
    );
    assert!(!pins.plugin_directories()[0].starts_with(&fixture.worktree));
}

#[test]
fn disabled_plugin_needs_no_installation_or_materialization() {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    fixture.set_settings(&settings(json!({"disabled@local": false})));
    assert!(
        fixture
            .prepare(&plan(&role), &role)
            .expect("disabled")
            .plugins
            .is_empty()
    );
}

#[test]
fn contained_dot_relative_resource_paths_keep_their_native_names() {
    let fixture = Fixture::new();
    let role = role("/review:worker");
    let original = fixture.plugin("review");
    fixture.set_settings(&settings(json!({"review@local": true})));
    write(
        &original.join("plugin.json"),
        br#"{"name":"review","version":"1.0.0","agents":"./agents","skills":"./skills"}"#,
    );
    let pins = fixture
        .prepare(&plan(&role), &role)
        .expect("dot-relative resources");
    assert_eq!(
        (
            pins.expected_catalog.agents.contains_key("review:worker"),
            pins.expected_catalog.skills.contains_key("review:review")
        ),
        (true, true)
    );
}

#[test]
fn missing_run_role_snapshot_never_falls_back_to_available_original() {
    let fixture = Fixture::new();
    let role = role("/review:worker");
    let mut plan = plan(&role);
    fixture.plugin("review");
    fixture.set_settings(&settings(json!({"review@local": true})));
    fixture.pinned_role(&mut plan, &role);
    fs::remove_dir_all(fixture.run.join("plugins/review")).expect("remove run pin");
    assert!(
        fixture
            .prepare(&plan, &role)
            .expect_err("missing run pin")
            .contains("missing")
    );
}

#[test]
fn private_context_cannot_be_placed_inside_the_worktree() {
    let mut fixture = Fixture::new();
    fixture.private = fixture.worktree.join("private");
    let role = role("worker.agent.md");
    assert!(fixture.prepare(&plan(&role), &role).is_err());
}
