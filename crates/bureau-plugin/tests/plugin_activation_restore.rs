//! Offline temporary activation and exact restoration tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bureau_plugin::{Error, Resolver};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    worktree: PathBuf,
    run: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let next = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from("target/plugin-activation-tests")
            .join(format!("{label}-{}-{next}", std::process::id()));
        let fixture = Self {
            run: root.join("run"),
            worktree: root.join("run/wt"),
            home: root.join("copilot"),
            root,
        };
        for path in [&fixture.worktree, &fixture.run, &fixture.home] {
            fs::create_dir_all(path).expect("create fixture");
        }
        fixture
    }

    fn resolver(&self) -> Resolver {
        Resolver::new(&self.run, Some(self.home.clone()))
    }

    fn install(&self, agent: &[u8]) {
        let plugin = self.home.join("installed-plugins/user/demo");
        write_plugin(&plugin, agent);
        let config = serde_json::json!({
            "installedPlugins": [{
                "name": "demo",
                "marketplace": "user",
                "cache_path": plugin.canonicalize().expect("canonical plugin"),
                "enabled": true
            }]
        });
        write_json(&self.home.join("config.json"), &config);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove fixture");
    }
}

#[test]
fn absent_marketplace_creates_ai_and_materializes_both_agents() {
    let fixture = Fixture::new("new-market");
    fixture.install(b"verbatim agent");
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    write(
        &temporary_plugin_path(&fixture).join("runtime-created.txt"),
        b"temporary",
    );
    let active = new_marketplace_state(&fixture);
    activation.restore().expect("restore");
    assert_eq!(
        (active, entries(&fixture.worktree)),
        ((true, true, true, true), BTreeMap::new())
    );
}

#[test]
fn alphabetically_first_existing_marketplace_is_injected_then_restored() {
    let fixture = Fixture::new("existing-market");
    fixture.install(b"agent");
    existing_marketplaces(&fixture);
    write(
        &fixture.worktree.join(".github/agents/reviewer.agent.md"),
        b"original copilot",
    );
    write(
        &fixture.worktree.join(".claude/agents/reviewer.md"),
        b"original claude",
    );
    let before = entries(&fixture.worktree);
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    let active = injected_catalog_state(&fixture);
    activation.restore().expect("restore");
    assert_eq!((active, entries(&fixture.worktree)), ((true, true), before));
}

#[test]
fn conflicting_agent_edit_escalates_after_restoring_originals() {
    let fixture = Fixture::new("conflict");
    fixture.install(b"agent");
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    fs::write(
        fixture.worktree.join(".github/agents/reviewer.agent.md"),
        b"agent edit",
    )
    .expect("edit activation file");
    let result = activation.restore();
    let seen = (
        matches!(result, Err(Error::Conflict { .. })),
        entries(&fixture.worktree),
    );
    assert_eq!(seen, (true, BTreeMap::new()));
}

#[test]
fn dropping_activation_performs_best_effort_restoration() {
    let fixture = Fixture::new("drop");
    fixture.install(b"agent");
    {
        let _activation = fixture
            .resolver()
            .activate("/demo:reviewer", &fixture.worktree)
            .expect("activate");
    }
    assert_eq!(entries(&fixture.worktree), BTreeMap::new());
}

#[test]
fn pinned_direct_agent_is_materialized_and_restored() {
    let fixture = Fixture::new("direct-agent");
    let activation = bureau_plugin::activate_direct(
        "agents/reviewer.md",
        b"verbatim agent",
        &fixture.worktree,
        &fixture.run,
    )
    .expect("activate");
    let active = materialized_agents(&fixture);
    activation.restore().expect("restore");
    assert_eq!(
        (active, entries(&fixture.worktree)),
        (true, BTreeMap::new())
    );
}

#[cfg(unix)]
#[test]
fn activation_rejects_existing_hard_linked_agent_file() {
    let fixture = Fixture::new("hard-link");
    fixture.install(b"agent");
    let agent = fixture.worktree.join(".github/agents/reviewer.agent.md");
    write(&agent, b"original");
    let outside = fixture.root.join("outside-agent");
    fs::hard_link(&agent, &outside).expect("hard link");
    let result = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree);
    assert!(result.is_err() && fs::read(outside).expect("outside") == b"original");
}

fn temporary_plugin_path(fixture: &Fixture) -> PathBuf {
    let catalog = read_json(&fixture.worktree.join(".ai/marketplace.json"));
    let source = catalog["plugins"][0]["source"]
        .as_str()
        .expect("plugin source");
    fixture.worktree.join(".ai").join(source)
}

fn new_marketplace_state(fixture: &Fixture) -> (bool, bool, bool, bool) {
    let settings = read_json(&fixture.worktree.join(".github/copilot/settings.json"));
    let catalog = read_json(&fixture.worktree.join(".ai/marketplace.json"));
    let source = catalog["plugins"][0]["source"]
        .as_str()
        .expect("plugin source");
    (
        settings["extraKnownMarketplaces"]["repo-plugins"]["source"]["path"] == ".ai",
        settings["enabledPlugins"]["demo@repo-plugins"] == true,
        fixture.worktree.join(".ai").join(source).is_dir(),
        materialized_agents(fixture),
    )
}

fn materialized_agents(fixture: &Fixture) -> bool {
    let copilot = fs::read(fixture.worktree.join(".github/agents/reviewer.agent.md"));
    let claude = fs::read(fixture.worktree.join(".claude/agents/reviewer.md"));
    matches!((copilot, claude), (Ok(left), Ok(right)) if left == b"verbatim agent" && right == left)
}

fn existing_marketplaces(fixture: &Fixture) {
    let settings = b"{\n // preserve this comment exactly\n \"extraKnownMarketplaces\": {\n  \"zeta\": {\"source\":{\"source\":\"directory\",\"path\":\"zeta\"}},\n  \"alpha\": {\"source\":{\"source\":\"directory\",\"path\":\"alpha\"}}\n },\n \"enabledPlugins\": {}\n}\n";
    write(
        &fixture.worktree.join(".github/copilot/settings.json"),
        settings,
    );
    write_catalog(&fixture.worktree.join("alpha"), "alpha");
    write_catalog(&fixture.worktree.join("zeta"), "zeta");
}

fn write_catalog(root: &Path, name: &str) {
    let value = serde_json::json!({
        "name": name,
        "plugins": [{ "name": "other", "source": "plugins/other" }]
    });
    write_json(&root.join("marketplace.json"), &value);
}

fn injected_catalog_state(fixture: &Fixture) -> (bool, bool) {
    let alpha = read_json(&fixture.worktree.join("alpha/marketplace.json"));
    let zeta = read_json(&fixture.worktree.join("zeta/marketplace.json"));
    let alpha_has_demo = alpha["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .any(|entry| entry["name"] == "demo");
    let zeta_has_demo = zeta["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .any(|entry| entry["name"] == "demo");
    (alpha_has_demo, !zeta_has_demo)
}

fn write_plugin(root: &Path, agent: &[u8]) {
    write_json(
        &root.join("plugin.json"),
        &serde_json::json!({ "name": "demo", "version": "1.0.0" }),
    );
    write(&root.join("agents/reviewer.agent.md"), agent);
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).expect("read json")).expect("parse json")
}

fn write_json(path: &Path, value: &serde_json::Value) {
    write(
        path,
        &serde_json::to_vec_pretty(value).expect("serialize json"),
    );
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    fs::write(path, bytes).expect("write fixture");
}

fn entries(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    collect_entries(root, root, &mut found);
    found
}

fn collect_entries(root: &Path, directory: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
    let mut paths = fs::read_dir(directory)
        .expect("read directory")
        .map(|entry| entry.expect("directory entry").path())
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_entries(root, &path, found);
        } else {
            let relative = path.strip_prefix(root).expect("relative").to_path_buf();
            found.insert(relative, fs::read(path).expect("read file"));
        }
    }
}
