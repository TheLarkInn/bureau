use std::fs;

use serde_json::json;

use crate::adapters::copilot_factory::context_types::PinnedContext;

use super::super::restore;
use super::support::{Fixture, plan, role, settings, write};

fn context(fixture: &Fixture) -> PinnedContext {
    let role = role("worker.agent.md");
    fixture.plugin("review");
    fixture.set_settings(&settings(json!({"review@local": true})));
    fixture
        .prepare(&plan(&role), &role)
        .expect("prepared context")
}

#[test]
fn restore_uses_only_saved_private_material_and_survives_ambient_changes() {
    let fixture = Fixture::new();
    let pins = context(&fixture);
    let before = serde_json::to_vec(&pins).expect("saved pins");
    let saved: PinnedContext = serde_json::from_slice(&before).expect("restored pins");
    fixture.set_settings(&json!({"hooks": "changed", "enabledPlugins": {"new@remote": true}}));
    write(
        &fixture.home.join("config.json"),
        b"not valid current settings",
    );
    fs::remove_dir_all(fixture.worktree.join("marketplace")).expect("remove sources");
    fs::remove_dir_all(fixture.run.join("plugins")).expect("remove old run copies");
    assert_eq!(
        (
            restore(&saved).is_ok(),
            serde_json::to_vec(&saved).expect("unchanged pins")
        ),
        (true, before)
    );
}

#[test]
fn changed_pin_fails_instead_of_recapturing_an_available_source() {
    let fixture = Fixture::new();
    let pins = context(&fixture);
    write(
        &pins.plugins[0].directory.join("agents/worker.agent.md"),
        b"Modified private body.",
    );
    assert!(
        restore(&pins)
            .expect_err("changed pin")
            .contains("digest mismatch")
    );
}

#[test]
fn missing_pin_fails_without_replacement() {
    let fixture = Fixture::new();
    let pins = context(&fixture);
    fs::remove_dir_all(&pins.plugins[0].directory).expect("remove pin");
    assert_eq!(
        (restore(&pins).is_err(), pins.plugins[0].directory.exists()),
        (true, false)
    );
}

#[test]
fn saved_catalog_cannot_silently_shrink_or_change_agent_selection() {
    let fixture = Fixture::new();
    let mut pins = context(&fixture);
    pins.expected_catalog.skills.clear();
    assert!(restore(&pins).is_err());
    pins.selected_agent = "other".to_owned();
    assert!(restore(&pins).is_err());
}

#[test]
fn a_source_path_cannot_be_substituted_for_a_private_copy() {
    let fixture = Fixture::new();
    let mut pins = context(&fixture);
    pins.plugins[0].directory = fixture.worktree.join("marketplace/plugins/review");
    assert!(
        restore(&pins)
            .expect_err("unprotected source")
            .contains("private location")
    );
}
