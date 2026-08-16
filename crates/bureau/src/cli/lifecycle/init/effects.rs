use bureau::setup::{
    ConfigDraft, ConfigPullRequest, ConfigSource, FirstPipeline, InitEffects, Merge,
    OutcomeSummary, PluginEffects, PluginSettings, ReconcilePass, Settings, SettingsEffects,
    ValidatedConfig,
};

use super::model::Request;
use super::proposal::Proposal;
use super::{author, draft, first_pass, merge, proposal, validate};

pub(super) struct LocalEffects {
    layout: bureau::home::Layout,
    request: Request,
    runtime: tokio::runtime::Handle,
    proposal: Option<Proposal>,
    outcomes: Option<OutcomeSummary>,
}

impl LocalEffects {
    pub(super) const fn new(
        layout: bureau::home::Layout,
        request: Request,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            layout,
            request,
            runtime,
            proposal: None,
            outcomes: None,
        }
    }
}

impl SettingsEffects for LocalEffects {
    type Error = Error;

    fn settings_exist(&mut self) -> Result<bool, Self::Error> {
        Ok(self.layout.settings().try_exists()?)
    }

    fn write_settings_atomically(&mut self, settings: &Settings) -> Result<(), Self::Error> {
        bureau::setup::save_settings(self.layout.settings(), settings)?;
        Ok(())
    }
}

impl PluginEffects for LocalEffects {
    type Error = Error;

    fn install_user_plugin(&mut self, _: &PluginSettings) -> Result<(), Self::Error> {
        super::super::setup::install_user_plugin(&self.runtime)
            .map_err(|error| Error(anyhow::Error::new(error)))
    }
}

impl InitEffects for LocalEffects {
    fn prepare_config(
        &mut self,
        selection: &FirstPipeline,
    ) -> Result<ConfigDraft, <Self as SettingsEffects>::Error> {
        let value = match selection {
            FirstPipeline::Fixed => draft::fixed(&self.request)?,
            FirstPipeline::AiAuthored { request } => {
                author::prepare(&self.layout, &self.request, request)?
            }
        };
        Ok(value)
    }

    fn preview_config(
        &mut self,
        _: &ConfigSource,
        draft: &ConfigDraft,
    ) -> Result<(), <Self as SettingsEffects>::Error> {
        validate::preview(draft);
        Ok(())
    }

    fn validate_config_preview(
        &mut self,
        draft: &ConfigDraft,
    ) -> Result<(), <Self as SettingsEffects>::Error> {
        Ok(validate::config(&self.layout, draft)?)
    }

    fn create_config_pull_request(
        &mut self,
        _: &ConfigSource,
        draft: &ConfigDraft,
    ) -> Result<ConfigPullRequest, <Self as SettingsEffects>::Error> {
        let created = self.runtime.block_on(proposal::create(
            &self.layout,
            &self.request.settings,
            draft,
        ))?;
        let pull_request = created.pull_request.clone();
        self.proposal = Some(created);
        Ok(pull_request)
    }

    fn wait_for_merge(
        &mut self,
        _: &ConfigPullRequest,
    ) -> Result<Merge, <Self as SettingsEffects>::Error> {
        let proposal = self
            .proposal
            .as_ref()
            .context("config proposal is missing")?;
        let commit = self
            .runtime
            .block_on(merge::wait(&self.request.settings, proposal))?;
        Ok(Merge { commit })
    }

    fn validate_merged_config(
        &mut self,
        _: &ConfigSource,
        commit: &str,
    ) -> Result<ValidatedConfig, <Self as SettingsEffects>::Error> {
        Ok(self.runtime.block_on(merge::validate(
            &self.layout,
            &self.request.settings,
            commit,
        ))?)
    }

    fn reconcile_once(
        &mut self,
        config: &ValidatedConfig,
    ) -> Result<ReconcilePass, <Self as SettingsEffects>::Error> {
        let outcomes = self.runtime.block_on(first_pass::run(
            &self.layout,
            &self.request.settings,
            config,
        ))?;
        self.outcomes = Some(outcomes);
        Ok(ReconcilePass {
            id: config.commit.clone(),
        })
    }

    fn wait_for_outcomes(
        &mut self,
        _: &ReconcilePass,
    ) -> Result<OutcomeSummary, <Self as SettingsEffects>::Error> {
        self.outcomes
            .take()
            .context("reconcile outcomes are missing")
            .map_err(Error::from)
    }
}

use anyhow::Context as _;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub(super) struct Error(#[from] anyhow::Error);

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self(value.into())
    }
}

impl From<bureau::setup::FileError> for Error {
    fn from(value: bureau::setup::FileError) -> Self {
        Self(value.into())
    }
}
