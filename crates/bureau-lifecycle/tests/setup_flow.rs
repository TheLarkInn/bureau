//! Offline tests for local settings changes.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use bureau_lifecycle::setup::{
    ConfigSource, Credential, CredentialSource, FlowError, MigrationEffects, MigrationSettings,
    PluginEffects, PluginSettings, Settings, SettingsEffects, SetupFlow, SetupState,
};

#[derive(Default)]
struct FakeEffects {
    exists: bool,
    events: Vec<&'static str>,
    written: Option<Settings>,
}

impl SettingsEffects for FakeEffects {
    type Error = io::Error;

    fn settings_exist(&mut self) -> io::Result<bool> {
        self.events.push("settings_exist");
        Ok(self.exists)
    }

    fn write_settings_atomically(&mut self, settings: &Settings) -> io::Result<()> {
        self.events.push("write_settings_atomically");
        self.written = Some(settings.clone());
        Ok(())
    }
}

impl PluginEffects for FakeEffects {
    type Error = io::Error;

    fn install_user_plugin(&mut self, _: &PluginSettings) -> io::Result<()> {
        self.events.push("install_user_plugin");
        Ok(())
    }
}

impl MigrationEffects for FakeEffects {
    type Error = io::Error;

    fn migrate_local_state(&mut self, _: &Settings) -> Result<(), Self::Error> {
        self.events.push("migrate_local_state");
        Ok(())
    }
}

fn config_source(single: bool) -> ConfigSource {
    if single {
        ConfigSource::SingleRepository {
            remote: "https://example.test/work.git".to_owned(),
            reference: "main".to_owned(),
        }
    } else {
        ConfigSource::SeparateRepository {
            remote: "https://example.test/config.git".to_owned(),
            reference: "reviewed".to_owned(),
        }
    }
}

fn credential_sources() -> BTreeMap<String, Credential> {
    BTreeMap::from([
        (
            "environment".to_owned(),
            Credential::new(CredentialSource::Environment {
                variable: "BUREAU_CREDENTIAL_GITHUB".to_owned(),
            })
            .as_identity("bureau-bot"),
        ),
        (
            "directory".to_owned(),
            Credential::new(CredentialSource::Directory {
                path: PathBuf::from("/secrets"),
            }),
        ),
        (
            "file".to_owned(),
            Credential::new(CredentialSource::File {
                path: PathBuf::from("/run/credentials/github"),
            }),
        ),
    ])
}

fn settings(single: bool, install: bool) -> Settings {
    Settings {
        config: config_source(single),
        credentials: credential_sources(),
        plugin: PluginSettings {
            install_user_global: install,
        },
        migration: MigrationSettings {
            source: Some(PathBuf::from("/old/bureau")),
        },
    }
}

#[test]
fn setup_changes_existing_settings_atomically() {
    let changed = settings(false, true);
    let mut flow = SetupFlow::new(changed.clone());
    let mut effects = FakeEffects {
        exists: true,
        ..FakeEffects::default()
    };
    flow.run(&mut effects).expect("setup succeeds");
    assert_eq!(
        effects.events,
        [
            "settings_exist",
            "install_user_plugin",
            "migrate_local_state",
            "write_settings_atomically"
        ]
    );
    let mut expected = changed;
    expected.migration.source = None;
    assert_eq!(effects.written, Some(expected));
    assert_eq!(flow.state(), SetupState::Complete);
}

#[test]
fn setup_requires_an_existing_installation() {
    let mut flow = SetupFlow::new(settings(true, false));
    let mut effects = FakeEffects::default();
    let error = flow.run(&mut effects).expect_err("setup requires init");
    let actual = (
        matches!(error, FlowError::SettingsMissing),
        effects.events,
        flow.state(),
    );
    assert_eq!(
        actual,
        (true, vec!["settings_exist"], SetupState::CheckingSettings)
    );
}

#[test]
fn plugin_installation_is_optional() {
    let mut flow = SetupFlow::new(settings(true, false));
    let mut effects = FakeEffects {
        exists: true,
        ..FakeEffects::default()
    };
    flow.run(&mut effects).expect("setup succeeds");
    assert_eq!(
        effects.events,
        [
            "settings_exist",
            "migrate_local_state",
            "write_settings_atomically"
        ]
    );
}

#[test]
fn serialized_settings_hold_references_not_secret_values() {
    let settings = settings(false, true);
    let yaml = serde_yaml_ng::to_string(&settings).expect("serialize settings");
    let decoded: Settings = serde_yaml_ng::from_str(&yaml).expect("deserialize settings");
    let source_names = ["environment", "directory", "file"]
        .into_iter()
        .all(|name| yaml.contains(&format!("source: {name}")));
    assert_eq!(decoded, settings);
    assert!(source_names, "{yaml}");
    assert!(!yaml.contains("actual-secret-value"), "{yaml}");
}

#[test]
fn config_sources_select_the_committed_config_directory() {
    let single = config_source(true);
    let separate = config_source(false);
    let actual = [
        (single.remote(), single.reference(), single.subdirectory()),
        (
            separate.remote(),
            separate.reference(),
            separate.subdirectory(),
        ),
    ];
    let expected = [
        (
            "https://example.test/work.git",
            "main",
            Path::new(".bureau"),
        ),
        (
            "https://example.test/config.git",
            "reviewed",
            Path::new("."),
        ),
    ];
    assert_eq!(actual, expected);
}
