use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bureau_plugin::Resolver;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    run: PathBuf,
    worktree: PathBuf,
    home: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = PathBuf::from("target/plugin-activation-tests").join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let run = root.join("run");
        let worktree = run.join("wt");
        let home = root.join("copilot");
        let source = home.join("installed-plugins/user/demo");
        for path in [&worktree, &source] {
            fs::create_dir_all(path).expect("fixture directory");
        }
        write_plugin(&source, "1.0.0");
        install_record(&home, &source);
        Self {
            root,
            run,
            worktree,
            home,
            source,
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
fn concurrent_worktrees_hold_independent_restoration_records() {
    let fixture = Fixture::new("concurrent-records");
    let first = fixture.run.join("member-a/wt");
    let second = fixture.run.join("member-b/wt");
    fs::create_dir_all(&first).expect("first worktree");
    fs::create_dir_all(&second).expect("second worktree");
    let first = fixture
        .resolver()
        .activate("/demo:reviewer", &first)
        .expect("first");
    let second = fixture
        .resolver()
        .activate("/demo:reviewer", &second)
        .expect("second");
    let active = bureau_plugin::restoration_infos(&fixture.run)
        .expect("records")
        .len();
    first.restore().expect("restore first");
    second.restore().expect("restore second");
    let remaining = bureau_plugin::restoration_infos(&fixture.run).expect("records");
    assert_eq!((active, remaining.len()), (2, 0));
}

#[test]
fn stale_restore_rechecks_the_installed_plugin_version() {
    let fixture = Fixture::new("version-guard");
    let activation = fixture
        .resolver()
        .activate("/demo:reviewer", &fixture.worktree)
        .expect("activate");
    std::mem::forget(activation);
    write_plugin(&fixture.source, "2.0.0");
    let info = bureau_plugin::restoration_infos(&fixture.run)
        .expect("records")
        .pop()
        .expect("record");
    let changed =
        bureau_plugin::restore_stale(&fixture.run, &info.activation_id, "demo", "1.0.0").is_err();
    write_plugin(&fixture.source, "1.0.0");
    let restored =
        bureau_plugin::restore_stale(&fixture.run, &info.activation_id, "demo", "1.0.0").is_ok();
    assert_eq!(
        (info.installed_version, changed, restored),
        ("2.0.0".to_owned(), true, true)
    );
}

#[test]
fn interrupted_record_write_does_not_poison_discovery() {
    let fixture = Fixture::new("temporary-record");
    let directory = fixture.run.join("activations");
    fs::create_dir_all(&directory).expect("activation directory");
    let temporary = directory.join(format!("{}.json.tmp", "a".repeat(64)));
    fs::write(temporary, b"{incomplete").expect("temporary record");
    let records = bureau_plugin::restoration_infos(&fixture.run).expect("discovery");
    assert!(records.is_empty());
}

fn install_record(home: &Path, source: &Path) {
    let value = serde_json::json!({
        "installedPlugins": [{
            "name": "demo",
            "marketplace": "user",
            "cache_path": source.canonicalize().expect("canonical source"),
            "enabled": true
        }]
    });
    write_json(&home.join("config.json"), &value);
}

fn write_plugin(root: &Path, version: &str) {
    write_json(
        &root.join("plugin.json"),
        &serde_json::json!({"name": "demo", "version": version}),
    );
    write(&root.join("agents/reviewer.agent.md"), b"agent");
}

fn write_json(path: &Path, value: &serde_json::Value) {
    write(path, &serde_json::to_vec_pretty(value).expect("serialize"));
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
    fs::write(path, bytes).expect("write fixture");
}
