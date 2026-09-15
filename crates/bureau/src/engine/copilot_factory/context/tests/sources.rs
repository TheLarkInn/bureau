use serde_json::{Value, json};

use super::support::{Fixture, plan, role, settings};

fn rejected_settings(value: &Value) -> bool {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    fixture.set_settings(value);
    fixture.prepare(&plan(&role), &role).is_err()
}

#[test]
fn correct_plural_marketplace_key_is_required() {
    let value = json!({
        "extraKnownMarketplace": {"local": {"source": {"source": "directory", "path": "marketplace"}}},
        "enabledPlugins": {"review@local": true}
    });
    assert!(rejected_settings(&value));
}

#[test]
fn malformed_unknown_and_remote_enabled_sources_fail_closed() {
    let cases = [
        json!({"enabledPlugins": {"review": true}}),
        json!({"enabledPlugins": {"review@unknown": true}}),
        json!({"enabledPlugins": {"review@local": "yes"}}),
        json!({"enabledPlugins": []}),
        json!({"extraKnownMarketplaces": []}),
        json!({"extraKnownMarketplaces": {"local": {"source": {"source": "github", "repo": "owner/repo"}}},
            "enabledPlugins": {"review@local": true}}),
        json!({"extraKnownMarketplaces": {"local": {"source": {"source": "directory", "path": "marketplace", "unknown": true}}},
            "enabledPlugins": {"review@local": true}}),
        settings(json!({"review@local": true})),
    ];
    for value in cases {
        assert!(rejected_settings(&value), "{value}");
    }
}

#[test]
fn local_catalog_needs_exactly_one_materialized_contained_entry() {
    let cases = [
        json!([]),
        json!([{"name": "review", "source": "missing"}]),
        json!([{"name": "review", "source": {"source": "github", "repo": "owner/repo"}}]),
        json!([{"name": "review", "source": "../outside"}]),
        json!([{"name": "review", "source": "plugins/review"}, {"name": "review", "source": "plugins/review"}]),
    ];
    for entries in cases {
        let fixture = Fixture::new();
        let role = role("worker.agent.md");
        fixture.plugin("review");
        fixture.catalog(&entries);
        fixture.set_settings(&settings(json!({"review@local": true})));
        assert!(fixture.prepare(&plan(&role), &role).is_err());
    }
}

#[test]
fn repository_defaults_cannot_resolve_unlisted_global_plugins() {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    fixture.set_settings(&json!({"enabledPlugins": {"review@global": true}}));
    assert!(fixture.prepare(&plan(&role), &role).is_err());
}

#[test]
fn escaping_marketplace_symlinks_are_rejected_without_reading_them() {
    let fixture = Fixture::new();
    let role = role("worker.agent.md");
    std::os::unix::fs::symlink(&fixture.home, fixture.worktree.join("marketplace"))
        .expect("fixture link");
    fixture.set_settings(&settings(json!({"review@local": true})));
    assert!(fixture.prepare(&plan(&role), &role).is_err());
}
