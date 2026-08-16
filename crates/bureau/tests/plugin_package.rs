//! Installable plugin and marketplace layout contract.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn json(path: &str) -> Value {
    let bytes = std::fs::read(root().join(path)).expect("read json");
    serde_json::from_slice(&bytes).expect("parse json")
}

#[test]
fn official_marketplace_exposes_the_three_plugins() {
    let market = json(".github/plugin/marketplace.json");
    let entries = market["plugins"].as_array().expect("plugins");
    let sources: BTreeMap<_, _> = entries
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().expect("name"),
                entry["source"].as_str().expect("source"),
            )
        })
        .collect();
    assert_eq!(
        sources,
        BTreeMap::from([
            ("bureau", "plugins/bureau"),
            ("language-behaviors", "plugins/language-behaviors"),
            ("rust-design-patterns", "plugins/rust-design-patterns"),
        ])
    );
}

#[test]
fn repository_settings_enable_the_official_marketplace() {
    let settings = json(".github/copilot/settings.json");
    let seen = (
        settings["extraKnownMarketplaces"]["bureau"]["source"]["path"].as_str(),
        settings["enabledPlugins"]["bureau@bureau"].as_bool(),
    );
    assert_eq!(seen, (Some("."), Some(true)));
}

#[test]
fn bureau_plugin_declares_agents_skills_and_mcp() {
    let plugin = json("plugins/bureau/plugin.json");
    let mcp = json("plugins/bureau/.mcp.json");
    let seen = (
        plugin["agents"].as_str(),
        plugin["skills"].as_str(),
        plugin["mcpServers"].as_str(),
        mcp["mcpServers"]["bureau-io"]["command"].as_str(),
        mcp["mcpServers"]["bureau-io"]["args"]
            .as_array()
            .map(Vec::len),
    );
    assert_eq!(
        seen,
        (
            Some("agents/"),
            Some("skills/"),
            Some(".mcp.json"),
            Some("bureau"),
            Some(2),
        )
    );
}

#[test]
fn bureau_resources_are_present_with_owned_models() {
    let implementer =
        std::fs::read_to_string(root().join("plugins/bureau/agents/implementer.agent.md"))
            .expect("implementer");
    let reviewer = std::fs::read_to_string(root().join("plugins/bureau/agents/reviewer.agent.md"))
        .expect("reviewer");
    let paths = [
        "plugins/bureau/skills/pipeline-author/SKILL.md",
        "plugins/bureau/skills/run-inspector/SKILL.md",
    ];
    let seen = (
        implementer.contains("model: opus"),
        reviewer.contains("model: sonnet"),
        paths.iter().all(|path| root().join(path).is_file()),
    );
    assert_eq!(seen, (true, true, true));
}
