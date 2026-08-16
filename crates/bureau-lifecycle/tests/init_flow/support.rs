use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use crate::setup::{
    ConfigDraft, ConfigPullRequest, ConfigSource, FirstPipeline, InitEffects, InitRequest, Merge,
    MigrationEffects, MigrationSettings, Outcome, OutcomeSummary, PluginEffects, PluginSettings,
    ReconcilePass, RunSummary, Settings, SettingsEffects, ValidatedConfig,
};

pub struct FakeEffects {
    settings_exist: bool,
    pub fail_at: Option<&'static str>,
    pub validated_commit: String,
    pub events: Vec<String>,
    pub proposed: Option<ConfigDraft>,
}

impl FakeEffects {
    pub fn new(settings_exist: bool) -> Self {
        Self {
            settings_exist,
            fail_at: None,
            validated_commit: "merge-7".to_owned(),
            events: Vec::new(),
            proposed: None,
        }
    }

    fn record(&mut self, event: &str) -> io::Result<()> {
        self.events.push(event.to_owned());
        match self.fail_at {
            Some(failure) if event.starts_with(failure) => Err(io::Error::other(event)),
            _ => Ok(()),
        }
    }
}

impl SettingsEffects for FakeEffects {
    type Error = io::Error;

    fn settings_exist(&mut self) -> io::Result<bool> {
        self.record("settings_exist")?;
        Ok(self.settings_exist)
    }

    fn write_settings_atomically(&mut self, _: &Settings) -> io::Result<()> {
        self.record("write_settings_atomically")
    }
}

impl PluginEffects for FakeEffects {
    type Error = io::Error;

    fn install_user_plugin(&mut self, _: &PluginSettings) -> io::Result<()> {
        self.record("install_user_plugin")
    }
}

impl MigrationEffects for FakeEffects {
    type Error = io::Error;

    fn migrate_local_state(&mut self, _: &Settings) -> io::Result<()> {
        self.record("migrate_local_state")
    }
}

impl InitEffects for FakeEffects {
    fn prepare_config(&mut self, selection: &FirstPipeline) -> io::Result<ConfigDraft> {
        match selection {
            FirstPipeline::Fixed => self.record("prepare_config:fixed")?,
            FirstPipeline::AiAuthored { request } => {
                self.record(&format!("prepare_config:ai:{request}"))?;
            }
        }
        Ok(config_draft(selection))
    }

    fn preview_config(&mut self, source: &ConfigSource, draft: &ConfigDraft) -> io::Result<()> {
        let path = source.subdirectory().display();
        self.record(&format!("preview_config:{path}:{}", draft.files.len()))
    }

    fn validate_config_preview(&mut self, _: &ConfigDraft) -> io::Result<()> {
        self.record("validate_config_preview")
    }

    fn create_config_pull_request(
        &mut self,
        source: &ConfigSource,
        draft: &ConfigDraft,
    ) -> io::Result<ConfigPullRequest> {
        let path = source.subdirectory().display();
        self.record(&format!("create_config_pull_request:{path}"))?;
        self.proposed = Some(draft.clone());
        Ok(ConfigPullRequest {
            id: "pr-7".to_owned(),
        })
    }

    fn wait_for_merge(&mut self, pull_request: &ConfigPullRequest) -> io::Result<Merge> {
        self.record(&format!("wait_for_merge:{}", pull_request.id))?;
        Ok(Merge {
            commit: "merge-7".to_owned(),
        })
    }

    fn validate_merged_config(
        &mut self,
        source: &ConfigSource,
        commit: &str,
    ) -> io::Result<ValidatedConfig> {
        self.record(&format!("validate_merged_config:{commit}"))?;
        Ok(ValidatedConfig {
            source: source.clone(),
            commit: self.validated_commit.clone(),
        })
    }

    fn reconcile_once(&mut self, config: &ValidatedConfig) -> io::Result<ReconcilePass> {
        self.record(&format!("reconcile_once:{}", config.commit))?;
        Ok(ReconcilePass {
            id: "pass-7".to_owned(),
        })
    }

    fn wait_for_outcomes(&mut self, pass: &ReconcilePass) -> io::Result<OutcomeSummary> {
        self.record(&format!("wait_for_outcomes:{}", pass.id))?;
        Ok(OutcomeSummary {
            runs: vec![RunSummary {
                run_id: "run-7".to_owned(),
                outcome: Outcome::Success,
            }],
        })
    }
}

pub fn source(single: bool) -> ConfigSource {
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

fn settings(config: ConfigSource, install: bool) -> Settings {
    Settings {
        config,
        credentials: BTreeMap::new(),
        plugin: PluginSettings {
            install_user_global: install,
        },
        migration: MigrationSettings::default(),
    }
}

pub fn config_draft(selection: &FirstPipeline) -> ConfigDraft {
    let contents = match selection {
        FirstPipeline::Fixed => "fixed-pipeline",
        FirstPipeline::AiAuthored { .. } => "ai-pipeline",
    };
    ConfigDraft {
        files: BTreeMap::from([(
            PathBuf::from("pipelines/first.yaml"),
            contents.as_bytes().to_vec(),
        )]),
    }
}

pub fn request(config: ConfigSource, first_pipeline: FirstPipeline, install: bool) -> InitRequest {
    InitRequest {
        settings: settings(config, install),
        first_pipeline,
    }
}

pub const fn expected_events() -> [&'static str; 11] {
    [
        "settings_exist",
        "install_user_plugin",
        "prepare_config:fixed",
        "preview_config:.bureau:1",
        "validate_config_preview",
        "create_config_pull_request:.bureau",
        "wait_for_merge:pr-7",
        "validate_merged_config:merge-7",
        "reconcile_once:merge-7",
        "wait_for_outcomes:pass-7",
        "write_settings_atomically",
    ]
}

pub const fn expected_authored_events() -> [&'static str; 10] {
    [
        "settings_exist",
        "prepare_config:ai:author the first pipeline",
        "preview_config:.:1",
        "validate_config_preview",
        "create_config_pull_request:.",
        "wait_for_merge:pr-7",
        "validate_merged_config:merge-7",
        "reconcile_once:merge-7",
        "wait_for_outcomes:pass-7",
        "write_settings_atomically",
    ]
}

pub fn has_run_effects(events: &[String]) -> bool {
    events
        .iter()
        .any(|event| event.starts_with("write_") || event.starts_with("reconcile_"))
}
