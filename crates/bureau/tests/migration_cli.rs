//! Offline explicit local-state migration acceptance.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::setup::{
    ConfigSource, MigrationSettings, PluginSettings, Settings, load_settings, save_settings,
};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-migration-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("fixture");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn setup_imports_durable_state_without_disposable_worktrees() {
    let fixture = Fixture::new("success");
    let source = fixture.path().join("source");
    seed_source(&source);
    let home = fixture.path().join("home");
    let request = fixture.path().join("settings.yaml");
    seed_target(&home, &request, Some(source));
    let output = setup(&home, &request);
    assert!(output.status.success() && imported_state_is_valid(&home));
}

fn imported_state_is_valid(home: &Path) -> bool {
    let state = home.join("state.db");
    let mode = std::fs::metadata(&state)
        .expect("state metadata")
        .permissions()
        .mode();
    let saved = load_settings(&home.join("settings.yaml")).expect("saved settings");
    row_count(&state, "runs") == 1
        && home.join("runs/old-run/events.jsonl").is_file()
        && !home.join("runs/old-run/wt").exists()
        && !home
            .join("runs/old-run/concurrent/group/member/wt")
            .exists()
        && !home.join("runs/old-run/activations").exists()
        && mode & 0o600 == 0o600
        && saved.migration.source.is_none()
}

#[test]
fn newer_database_is_rejected_without_partial_import() {
    let fixture = Fixture::new("newer");
    let source = fixture.path().join("source");
    std::fs::create_dir_all(&source).expect("source");
    rusqlite::Connection::open(source.join("state.db"))
        .and_then(|connection| connection.execute("CREATE TABLE future(value TEXT)", []))
        .expect("newer state");
    let home = fixture.path().join("home");
    let request = fixture.path().join("settings.yaml");
    seed_target(&home, &request, Some(source));
    let output = setup(&home, &request);
    let saved = load_settings(&home.join("settings.yaml")).expect("saved settings");
    assert!(
        !output.status.success()
            && !home.join("state.db").exists()
            && saved.config.remote() == "old"
    );
}

#[test]
fn source_with_an_active_lease_is_rejected() {
    let fixture = Fixture::new("active");
    let source = fixture.path().join("source");
    std::fs::create_dir_all(&source).expect("source");
    let store = bureau::state::Store::open(&source.join("state.db")).expect("state");
    store
        .try_claim_run(
            "assignment",
            "github",
            "42",
            "active-run",
            std::time::Duration::from_secs(60),
        )
        .expect("active lease");
    drop(store);
    let home = fixture.path().join("home");
    let request = fixture.path().join("settings.yaml");
    seed_target(&home, &request, Some(source));
    let output = setup(&home, &request);
    assert!(!output.status.success() && !home.join("state.db").exists());
}

#[test]
fn source_with_a_pending_migration_is_rejected() {
    let fixture = Fixture::new("source-pending");
    let source = fixture.path().join("source");
    seed_source(&source);
    std::fs::write(source.join("migration.json"), "{}").expect("pending marker");
    let home = fixture.path().join("home");
    let request = fixture.path().join("settings.yaml");
    seed_target(&home, &request, Some(source));
    let output = setup(&home, &request);
    assert!(!output.status.success() && !home.join("state.db").exists());
}

#[test]
fn settings_failure_rolls_back_imported_state() {
    let fixture = Fixture::new("rollback");
    let source = fixture.path().join("source");
    seed_source(&source);
    let home = fixture.path().join("home");
    let request = fixture.path().join("settings.yaml");
    seed_target(&home, &request, Some(source));
    std::fs::create_dir(home.join("settings.yaml.tmp")).expect("blocked temporary");
    let output = setup(&home, &request);
    let saved = load_settings(&home.join("settings.yaml")).expect("old settings");
    let clean = !home.join("state.db").exists() && !home.join("runs").exists();
    assert!(!output.status.success() && clean && saved.config.remote() == "old");
}

#[cfg(unix)]
#[test]
fn symlinked_source_is_rejected_without_partial_import() {
    let fixture = Fixture::new("symlink");
    let source = fixture.path().join("source");
    seed_source(&source);
    let link = fixture.path().join("source-link");
    std::os::unix::fs::symlink(&source, &link).expect("source symlink");
    let home = fixture.path().join("home");
    let request = fixture.path().join("settings.yaml");
    seed_target(&home, &request, Some(link));
    let output = setup(&home, &request);
    assert!(!output.status.success() && !home.join("state.db").exists());
}

fn seed_source(source: &Path) {
    std::fs::create_dir_all(source).expect("source");
    let store = bureau::state::Store::open(&source.join("state.db")).expect("state");
    store
        .record_run("old-run", "assignment", 1.5)
        .expect("record run");
    drop(store);
    let mut log =
        bureau::runlog::RunLog::create(&source.join("runs"), "old-run", &[]).expect("run log");
    log.append(
        bureau::runlog::EventKind::RunStarted,
        bureau::runlog::run_started("old-run", "assignment"),
    )
    .expect("start");
    let run = log.dir().to_path_buf();
    log.close().expect("close");
    std::fs::create_dir_all(run.join("wt")).expect("worktree");
    std::fs::write(run.join("wt/disposable"), "discard").expect("worktree file");
    std::fs::create_dir(run.join("activations")).expect("activations");
    std::fs::write(run.join("activations/stale.json"), "{}").expect("activation record");
    let nested = run.join("concurrent/group/member/wt");
    std::fs::create_dir_all(&nested).expect("nested worktree");
    std::fs::write(nested.join("disposable"), "discard").expect("nested file");
}

fn seed_target(home: &Path, request: &Path, source: Option<PathBuf>) {
    std::fs::create_dir_all(home).expect("home");
    save_settings(&home.join("settings.yaml"), &settings("old", None)).expect("old settings");
    save_settings(request, &settings("new", source)).expect("new settings");
}

fn settings(remote: &str, source: Option<PathBuf>) -> Settings {
    Settings {
        config: ConfigSource::SeparateRepository {
            remote: remote.to_owned(),
            reference: "main".to_owned(),
        },
        credentials: BTreeMap::new(),
        plugin: PluginSettings::default(),
        migration: MigrationSettings { source },
    }
}

fn setup(home: &Path, request: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(["setup", "--from", &request.to_string_lossy()])
        .env("BUREAU_HOME", home)
        .output()
        .expect("bureau setup")
}

fn row_count(path: &Path, table: &str) -> i64 {
    rusqlite::Connection::open(path)
        .and_then(|connection| {
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
        })
        .expect("row count")
}
