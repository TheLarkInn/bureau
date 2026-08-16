//! Offline plugin snapshot integrity and path-safety tests.

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

    fn plugin(&self) -> PathBuf {
        self.home.join("installed-plugins/user/demo")
    }

    fn install(&self, enabled: bool) {
        let plugin = self.plugin();
        let config = serde_json::json!({
            "installedPlugins": [{
                "name": "demo",
                "marketplace": "user",
                "cache_path": plugin.canonicalize().expect("canonical plugin"),
                "enabled": enabled
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
fn deterministic_digest_ignores_creation_order() {
    let first = Fixture::new("digest-first");
    let second = Fixture::new("digest-second");
    write_plugin(&first.plugin(), false);
    write_plugin(&second.plugin(), true);
    first.install(true);
    second.install(true);
    let left = activate_digest(&first);
    let right = activate_digest(&second);
    assert_eq!(left, right);
}

#[cfg(unix)]
#[test]
fn digest_changes_when_executable_permissions_change() {
    use std::os::unix::fs::PermissionsExt as _;

    let first = Fixture::new("mode-first");
    let second = Fixture::new("mode-second");
    write_plugin(&first.plugin(), false);
    write_plugin(&second.plugin(), false);
    let agent = second.plugin().join("agents/reviewer.agent.md");
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).expect("chmod");
    first.install(true);
    second.install(true);
    assert_ne!(activate_digest(&first), activate_digest(&second));
}

#[test]
fn changed_durable_snapshot_is_rejected_on_resume() {
    let fixture = Fixture::new("snapshot-validation");
    write_plugin(&fixture.plugin(), false);
    fixture.install(true);
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    activation.restore().expect("restore");
    fs::write(
        fixture
            .run
            .join("plugins/demo/tree/agents/reviewer.agent.md"),
        b"changed",
    )
    .expect("change snapshot");
    let result = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree);
    assert!(matches!(result, Err(Error::InvalidData { .. })));
}

#[test]
fn stale_staging_directory_does_not_poison_snapshot_creation() {
    let fixture = Fixture::new("stale-stage");
    write_plugin(&fixture.plugin(), false);
    fixture.install(true);
    let plugins = fixture.run.join("plugins");
    fs::create_dir_all(&plugins).expect("plugins");
    fs::create_dir(plugins.join(".demo-copy-stale")).expect("stale");
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    activation.restore().expect("restore");
    assert!(plugins.join("demo/source.json").is_file());
}

#[cfg(unix)]
#[test]
fn plugin_tree_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("symlink");
    write_plugin(&fixture.plugin(), false);
    symlink("plugin.json", fixture.plugin().join("manifest-link")).expect("create plugin symlink");
    fixture.install(true);
    let result = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree);
    assert!(matches!(result, Err(Error::InvalidData { .. })));
}

#[cfg(unix)]
#[test]
fn restoration_does_not_follow_replaced_parent_symlink() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("restore-parent-symlink");
    write_plugin(&fixture.plugin(), false);
    fixture.install(true);
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    let agents = fixture.worktree.join(".github/agents");
    fs::remove_file(agents.join("reviewer.agent.md")).expect("remove injected");
    fs::remove_dir(&agents).expect("remove agents");
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).expect("outside");
    let victim = outside.join("reviewer.agent.md");
    fs::write(&victim, b"keep").expect("victim");
    symlink(&outside, &agents).expect("replace parent");
    let result = activation.restore();
    let preserved = fs::read(&victim).expect("victim");
    assert!(result.is_err() && preserved == b"keep");
}

#[cfg(unix)]
#[test]
fn missing_run_below_worktree_symlink_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("run-parent-symlink");
    write_plugin(&fixture.plugin(), false);
    fixture.install(true);
    let linked = fixture.root.join("linked-run-parent");
    symlink(&fixture.worktree, &linked).expect("symlink");
    let result = Resolver::new(linked.join("new-run"), Some(fixture.home.clone()))
        .activate("/demo:reviewer", &fixture.worktree);
    assert!(matches!(result, Err(Error::InvalidData { .. })));
}

#[test]
fn conflict_message_reports_incomplete_restoration() {
    let error = Error::Conflict {
        paths: vec![PathBuf::from("agent.md")],
        restore_failures: vec!["permission denied".to_owned()],
    };
    let message = error.to_string();
    assert!(
        message.contains("restoration was incomplete") && message.contains("permission denied")
    );
}

#[test]
fn target_catalog_source_may_not_escape_marketplace() {
    let fixture = Fixture::new("path-escape");
    write_plugin(&fixture.worktree.join("outside"), false);
    let catalog = serde_json::json!({
        "name": "local",
        "plugins": [{ "name": "demo", "source": "../outside" }]
    });
    write_json(&fixture.worktree.join("market/marketplace.json"), &catalog);
    write_target_settings(&fixture);
    let result = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree);
    assert!(matches!(result, Err(Error::InvalidData { .. })));
}

#[test]
fn disabled_global_plugin_and_direct_agent_paths_are_not_resolved() {
    let fixture = Fixture::new("disabled");
    write_plugin(&fixture.plugin(), false);
    fixture.install(false);
    let missing = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree);
    let direct = fixture
        .resolver()
        .activate("agents/reviewer.agent.md", &fixture.worktree);
    let missing = missing.expect_err("disabled plugin").to_string();
    let direct = direct.expect_err("direct path").to_string();
    assert_eq!(
        (
            missing.starts_with("run `bureau setup`"),
            direct.starts_with("use a `/plugin:agent`")
        ),
        (true, true)
    );
}

fn write_target_settings(fixture: &Fixture) {
    let value = serde_json::json!({
        "extraKnownMarketplaces": {
            "local": { "source": { "source": "directory", "path": "market" } }
        },
        "enabledPlugins": { "demo@local": true }
    });
    write_json(
        &fixture.worktree.join(".github/copilot/settings.json"),
        &value,
    );
}

fn activate_digest(fixture: &Fixture) -> String {
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    let digest = activation.metadata().digest.clone();
    activation.restore().expect("restore");
    digest
}

fn write_plugin(root: &Path, reverse: bool) {
    let files = [
        (
            "plugin.json",
            br#"{"name":"demo","version":"1.0.0"}"#.as_slice(),
        ),
        ("agents/reviewer.agent.md", b"agent".as_slice()),
        ("data/a.txt", b"alpha".as_slice()),
        ("data/b.txt", b"beta".as_slice()),
    ];
    let indices: Vec<usize> = if reverse {
        (0..files.len()).rev().collect()
    } else {
        (0..files.len()).collect()
    };
    for index in indices {
        write(&root.join(files[index].0), files[index].1);
    }
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
