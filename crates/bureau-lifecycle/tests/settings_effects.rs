//! Offline production settings-effect tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau_lifecycle::home::Home;
use bureau_lifecycle::setup::{
    ConfigSource, FileEffects, FileError, MigrationEffects, MigrationSettings, PluginEffects,
    PluginSettings, Settings, SettingsEffects, SetupFlow, load_settings, save_settings,
};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct ProjectDir(PathBuf);

impl ProjectDir {
    fn new() -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/settings-effects");
        let path = root.join(format!("{}-{suffix}", std::process::id()));
        let _removed = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create project-local fixture");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ProjectDir {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn settings(reference: &str) -> Settings {
    Settings {
        config: ConfigSource::SeparateRepository {
            remote: "file:///config.git".to_owned(),
            reference: reference.to_owned(),
        },
        credentials: BTreeMap::new(),
        plugin: PluginSettings::default(),
        migration: MigrationSettings::default(),
    }
}

struct SetupFileEffects<'a>(FileEffects<'a>);

impl SettingsEffects for SetupFileEffects<'_> {
    type Error = FileError;

    fn settings_exist(&mut self) -> Result<bool, Self::Error> {
        self.0.settings_exist()
    }

    fn write_settings_atomically(&mut self, settings: &Settings) -> Result<(), Self::Error> {
        self.0.write_settings_atomically(settings)
    }
}

impl PluginEffects for SetupFileEffects<'_> {
    type Error = FileError;

    fn install_user_plugin(&mut self, _: &PluginSettings) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl MigrationEffects for SetupFileEffects<'_> {
    type Error = FileError;

    fn migrate_local_state(&mut self, _: &Settings) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn persisted_values() -> (bool, bool, Settings, bool) {
    let fixture = ProjectDir::new();
    let home = Home::new(fixture.path().join("home"));
    let path = home.layout().settings();
    let mut effects = FileEffects::new(home.layout());
    let before = effects.settings_exist().expect("inspect missing settings");
    effects
        .write_settings_atomically(&settings("first"))
        .expect("write initial settings");
    let replacement = settings("reviewed");
    effects
        .write_settings_atomically(&replacement)
        .expect("replace settings");
    let after = effects.settings_exist().expect("inspect saved settings");
    let loaded = load_settings(path).expect("load saved settings");
    (
        before,
        after,
        loaded,
        path.with_extension("yaml.tmp").exists(),
    )
}

#[test]
fn filesystem_effects_replace_settings_atomically() {
    let actual = persisted_values();
    assert_eq!(actual, (false, true, settings("reviewed"), false));
}

#[test]
fn setup_flow_uses_production_file_effects() {
    let fixture = ProjectDir::new();
    let home = Home::new(fixture.path().join("home"));
    save_settings(home.layout().settings(), &settings("first")).expect("seed settings");
    let expected = settings("reviewed");
    let mut flow = SetupFlow::new(expected.clone());
    flow.run(&mut SetupFileEffects(FileEffects::new(home.layout())))
        .expect("update settings");
    let loaded = load_settings(home.layout().settings()).expect("load updated settings");
    assert_eq!(loaded, expected);
}
