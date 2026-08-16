use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{Recovery, begin, data_moved, effects_running, finish, recover, start_commit};

static NEXT: AtomicU32 = AtomicU32::new(0);

#[test]
fn recovery_discards_a_prepared_only_transaction() {
    let fixture = fixture("prepared");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = stage(&fixture);
    let settings = settings();
    bureau::setup::save_settings(home.layout().settings(), &settings).expect("existing settings");
    begin(home.layout(), &stage, &settings, &fixture, false).expect("marker");
    let blocked = bureau::maintenance::shared(home.layout().root()).is_err();
    let recovered = recover(home.layout()).expect("recover");
    let clean = !stage.exists() && !fixture.join("migration.json").exists();
    cleanup(&fixture);
    assert_eq!(
        (recovered, clean, blocked),
        (Recovery::RolledBack, true, true)
    );
}

#[test]
fn recovery_rolls_back_moved_state_before_settings_commit() {
    let fixture = fixture("rollback");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = staged_durable(&fixture);
    begin(home.layout(), &stage, &settings(), &fixture, false).expect("marker");
    move_durable(&stage, home.layout());
    data_moved(home.layout(), false).expect("data moved");
    let recovered = recover(home.layout()).expect("recover");
    let clean = !home.layout().state_db().exists() && !home.layout().runs().exists();
    cleanup(&fixture);
    assert_eq!((recovered, clean), (Recovery::RolledBack, true));
}

#[test]
fn recovery_finishes_when_settings_were_committed() {
    let fixture = fixture("complete");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = staged_durable(&fixture);
    let settings = settings();
    begin(home.layout(), &stage, &settings, &fixture, false).expect("marker");
    move_durable(&stage, home.layout());
    data_moved(home.layout(), false).expect("data moved");
    bureau::setup::save_settings(home.layout().settings(), &settings).expect("settings");
    let recovered = recover(home.layout()).expect("recover");
    let complete = home.layout().state_db().exists() && home.layout().runs().exists();
    cleanup(&fixture);
    assert_eq!((recovered, complete), (Recovery::Completed, true));
}

#[test]
fn unreadable_settings_retain_the_recovery_marker() {
    let fixture = fixture("unreadable");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = staged_durable(&fixture);
    begin(home.layout(), &stage, &settings(), &fixture, false).expect("marker");
    move_durable(&stage, home.layout());
    data_moved(home.layout(), false).expect("data moved");
    std::fs::create_dir(home.layout().settings()).expect("settings directory");
    let failed = recover(home.layout()).is_err();
    let retained = fixture.join("migration.json").is_file();
    std::fs::remove_dir(home.layout().settings()).expect("remove settings directory");
    recover(home.layout()).expect("rollback");
    cleanup(&fixture);
    assert!(failed && retained);
}

#[test]
fn effects_running_preserves_the_staged_transaction() {
    let fixture = fixture("effects");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = staged_durable(&fixture);
    begin(home.layout(), &stage, &settings(), &fixture, false).expect("marker");
    effects_running(home.layout()).expect("effects running");
    let recovered = recover(home.layout()).expect("recover");
    let preserved = matches!(recovered, Recovery::Resume(_)) && stage.exists();
    finish(home.layout()).expect("finish");
    cleanup(&fixture);
    assert!(preserved);
}

#[test]
fn effects_commit_recovers_after_the_first_rename() {
    let fixture = fixture("effects-commit");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = staged_durable(&fixture);
    let settings = settings();
    begin(home.layout(), &stage, &settings, &fixture, false).expect("marker");
    effects_running(home.layout()).expect("effects running");
    assert!(start_commit(home.layout()).expect("start commit"));
    std::fs::rename(stage.join("state.db"), home.layout().state_db()).expect("move state");
    let recovered = recover(home.layout()).expect("recover");
    let complete = home.layout().runs().exists() && home.layout().settings().exists();
    cleanup(&fixture);
    assert_eq!((recovered, complete), (Recovery::Completed, true));
}

#[test]
fn effects_commit_allows_an_absent_optional_runs_payload() {
    let fixture = fixture("state-only");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = stage(&fixture);
    std::fs::write(stage.join("state.db"), "state").expect("state");
    let settings = settings();
    begin(home.layout(), &stage, &settings, &fixture, false).expect("marker");
    effects_running(home.layout()).expect("effects running");
    assert!(start_commit(home.layout()).expect("start commit"));
    std::fs::rename(stage.join("state.db"), home.layout().state_db()).expect("move state");
    let recovered = recover(home.layout()).expect("recover");
    let complete = home.layout().state_db().exists() && !home.layout().runs().exists();
    cleanup(&fixture);
    assert_eq!((recovered, complete), (Recovery::Completed, true));
}

#[test]
fn effects_data_moved_installs_missing_final_settings() {
    let fixture = fixture("effects-moved");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = staged_durable(&fixture);
    begin(home.layout(), &stage, &settings(), &fixture, false).expect("marker");
    effects_running(home.layout()).expect("effects running");
    assert!(start_commit(home.layout()).expect("start commit"));
    move_durable(&stage, home.layout());
    data_moved(home.layout(), true).expect("data moved");
    let recovered = recover(home.layout()).expect("recover");
    let complete = home.layout().settings().exists() && home.layout().runs().exists();
    cleanup(&fixture);
    assert_eq!((recovered, complete), (Recovery::Completed, true));
}

#[cfg(unix)]
#[test]
fn effects_commit_rejects_a_replaced_stage_symlink() {
    let fixture = fixture("stage-symlink");
    let home = bureau::home::Home::new(fixture.clone());
    let stage = staged_durable(&fixture);
    begin(home.layout(), &stage, &settings(), &fixture, false).expect("marker");
    effects_running(home.layout()).expect("effects running");
    assert!(start_commit(home.layout()).expect("start commit"));
    std::fs::remove_dir_all(&stage).expect("remove stage");
    let outside = fixture.join("outside");
    std::fs::create_dir(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, &stage).expect("stage symlink");
    let rejected = recover(home.layout()).is_err() && outside.exists();
    std::fs::remove_file(&stage).expect("remove symlink");
    std::fs::remove_file(fixture.join("migration.json")).expect("remove marker");
    cleanup(&fixture);
    assert!(rejected);
}

fn fixture(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bureau-migration-transaction-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _removed = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("fixture");
    path
}

fn stage(root: &Path) -> PathBuf {
    let stage = root.join(".migration-test");
    std::fs::create_dir(&stage).expect("stage");
    stage
}

fn staged_durable(root: &Path) -> PathBuf {
    let stage = stage(root);
    std::fs::write(stage.join("state.db"), "state").expect("state");
    std::fs::create_dir(stage.join("runs")).expect("runs");
    stage
}

fn move_durable(stage: &Path, layout: &bureau::home::Layout) {
    std::fs::rename(stage.join("state.db"), layout.state_db()).expect("move state");
    std::fs::rename(stage.join("runs"), layout.runs()).expect("move runs");
}

fn settings() -> bureau::setup::Settings {
    bureau::setup::Settings {
        config: bureau::setup::ConfigSource::SeparateRepository {
            remote: "config".to_owned(),
            reference: "main".to_owned(),
        },
        credentials: BTreeMap::new(),
        plugin: bureau::setup::PluginSettings::default(),
        migration: bureau::setup::MigrationSettings::default(),
    }
}

fn cleanup(path: &Path) {
    std::fs::remove_dir_all(path).expect("cleanup");
}
