//! Offline source-precedence and durable-resume tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bureau::plugin::Resolver;

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
        let worktree = root.join("worktree");
        let run = root.join("run");
        let home = root.join("copilot");
        for path in [&worktree, &run, &home] {
            fs::create_dir_all(path).expect("create fixture");
        }
        Self {
            root,
            worktree,
            run,
            home,
        }
    }

    fn resolver(&self) -> Resolver {
        Resolver::new(&self.run, Some(self.home.clone()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove fixture");
    }
}

#[test]
fn target_marketplace_overrides_user_global_plugin() {
    let fixture = Fixture::new("target-override");
    let global = global_plugin(&fixture, "demo", "1.0.0", b"global");
    install_record(&fixture, "demo", &global, true);
    local_marketplace(&fixture, "local", "demo", "2.0.0", b"target");
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate target");
    let seen = (
        activation.metadata().version.clone(),
        activation.metadata().source.clone(),
        activation.agent_name().to_owned(),
    );
    activation.restore().expect("restore");
    assert_eq!(
        seen,
        (
            "2.0.0".to_owned(),
            "target repository marketplace `local`".to_owned(),
            "reviewer".to_owned()
        )
    );
}

#[test]
fn user_global_plugin_is_the_second_source() {
    let fixture = Fixture::new("global-fallback");
    let global = global_plugin(&fixture, "demo", "1.2.3", b"global");
    install_record(&fixture, "demo", &global, true);
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate global");
    let seen = (
        activation.metadata().version.clone(),
        activation.metadata().source.clone(),
    );
    activation.restore().expect("restore");
    assert_eq!(
        seen,
        (
            "1.2.3".to_owned(),
            "user-global plugin `demo@user`".to_owned()
        )
    );
}

#[test]
fn development_bureau_plugin_is_the_last_source_when_present() {
    let fixture = Fixture::new("development-fallback");
    let activation = Resolver::new(&fixture.run, None)
        .activate("/bureau:implementer", &fixture.worktree)
        .expect("activate development plugin");
    let seen = (
        activation.metadata().name.clone(),
        activation.metadata().source.clone(),
    );
    activation.restore().expect("restore");
    assert_eq!(
        seen,
        (
            "bureau".to_owned(),
            "development source checkout".to_owned()
        )
    );
}

#[test]
fn missing_plugin_error_starts_with_an_action() {
    let fixture = Fixture::new("missing");
    let error = fixture
        .resolver()
        .activate("/not-installed:reviewer", &fixture.worktree)
        .expect_err("missing plugin");
    let message = error.to_string();
    assert!(message.starts_with("run `bureau setup` or install plugin"));
}

#[test]
fn durable_snapshot_is_reused_after_source_disappears() {
    let fixture = Fixture::new("resume");
    let global = global_plugin(&fixture, "demo", "1.0.0", b"first");
    install_record(&fixture, "demo", &global, true);
    let first = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("first activation");
    let first_source = first.restore().expect("first restore");
    fs::remove_dir_all(&fixture.home).expect("remove original source");
    let second_worktree = fixture.root.join("second-worktree");
    fs::create_dir(&second_worktree).expect("second worktree");
    let second = fixture
        .resolver()
        .activate("/demo:reviewer", &second_worktree)
        .expect("resume activation");
    let seen = (
        second.metadata() == &first_source,
        fs::read(second_worktree.join(".github/agents/reviewer.agent.md"))
            .expect("materialized agent"),
    );
    second.restore().expect("second restore");
    assert_eq!(seen, (true, b"first".to_vec()));
}

fn global_plugin(fixture: &Fixture, name: &str, version: &str, agent: &[u8]) -> PathBuf {
    let root = fixture
        .home
        .join("installed-plugins")
        .join("user")
        .join(name);
    write_plugin(&root, name, version, agent);
    root
}

fn install_record(fixture: &Fixture, name: &str, root: &Path, enabled: bool) {
    let value = serde_json::json!({
        "installedPlugins": [{
            "name": name,
            "marketplace": "user",
            "cache_path": root.canonicalize().expect("canonical plugin"),
            "enabled": enabled
        }]
    });
    write_json(&fixture.home.join("config.json"), &value);
}

fn local_marketplace(fixture: &Fixture, market: &str, plugin: &str, version: &str, agent: &[u8]) {
    let root = fixture.worktree.join("market");
    write_plugin(&root.join("plugins").join(plugin), plugin, version, agent);
    let catalog = serde_json::json!({
        "name": market,
        "plugins": [{ "name": plugin, "source": format!("plugins/{plugin}") }]
    });
    write_json(&root.join("marketplace.json"), &catalog);
    write_settings(fixture, market);
}

fn write_settings(fixture: &Fixture, market: &str) {
    let settings = format!(
        "{{\n /* local source */\n \"extraKnownMarketplaces\": {{\"{market}\": {{\"source\": {{\"source\": \"directory\", \"path\": \"market\"}}}}}},\n \"enabledPlugins\": {{\"demo@{market}\": true}}\n}}\n"
    );
    write(
        &fixture.worktree.join(".github/copilot/settings.json"),
        settings.as_bytes(),
    );
}

fn write_plugin(root: &Path, name: &str, version: &str, agent: &[u8]) {
    let manifest = serde_json::json!({ "name": name, "version": version });
    write_json(&root.join("plugin.json"), &manifest);
    write(&root.join("agents/reviewer.agent.md"), agent);
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
