use super::{
    ConfigDraft, ConfigPullRequest, ConfigSource, FirstPipeline, Merge, OutcomeSummary,
    PluginSettings, ReconcilePass, Settings, ValidatedConfig,
};

/// Local settings persistence effects.
pub trait SettingsEffects {
    /// Effect failure type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reports whether `settings.yaml` already exists.
    ///
    /// # Errors
    /// Returns an implementation error when local state cannot be inspected.
    fn settings_exist(&mut self) -> Result<bool, Self::Error>;

    /// Replaces settings through a same-directory temporary file and rename.
    ///
    /// # Errors
    /// Returns an implementation error when the atomic replacement fails.
    fn write_settings_atomically(&mut self, settings: &Settings) -> Result<(), Self::Error>;
}

/// User-global plugin installation effects.
pub trait PluginEffects {
    /// Effect failure type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Installs the bundled plugin for the current user.
    ///
    /// # Errors
    /// Returns an implementation error when installation fails.
    fn install_user_plugin(&mut self, settings: &PluginSettings) -> Result<(), Self::Error>;
}

/// Explicit local-state migration effects.
pub trait MigrationEffects {
    /// Effect failure type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Imports only the explicitly selected prior local state.
    ///
    /// # Errors
    /// Rejects unsafe, newer, overlapping, or non-empty migration targets.
    fn migrate_local_state(&mut self, settings: &Settings) -> Result<(), Self::Error>;
}

/// First-time initialization side effects.
pub trait InitEffects:
    SettingsEffects
    + PluginEffects<Error = <Self as SettingsEffects>::Error>
    + MigrationEffects<Error = <Self as SettingsEffects>::Error>
{
    /// Prepares the complete reference or AI-authored configuration draft.
    ///
    /// # Errors
    /// Returns an implementation error when authoring fails.
    fn prepare_config(
        &mut self,
        selection: &FirstPipeline,
    ) -> Result<ConfigDraft, <Self as SettingsEffects>::Error>;

    /// Presents the exact draft that validation and the pull request will use.
    ///
    /// # Errors
    /// Returns an implementation error when the preview cannot be presented.
    fn preview_config(
        &mut self,
        source: &ConfigSource,
        draft: &ConfigDraft,
    ) -> Result<(), <Self as SettingsEffects>::Error>;

    /// Validates the complete uncommitted preview without executing it.
    ///
    /// # Errors
    /// Returns an implementation error or validation failure.
    fn validate_config_preview(
        &mut self,
        draft: &ConfigDraft,
    ) -> Result<(), <Self as SettingsEffects>::Error>;

    /// Proposes the already-previewed and validated draft.
    ///
    /// # Errors
    /// Returns an implementation error when pull-request creation fails.
    fn create_config_pull_request(
        &mut self,
        source: &ConfigSource,
        draft: &ConfigDraft,
    ) -> Result<ConfigPullRequest, <Self as SettingsEffects>::Error>;

    /// Waits until the created pull request is merged.
    ///
    /// # Errors
    /// Returns an implementation error when the wait fails or the proposal closes.
    fn wait_for_merge(
        &mut self,
        pull_request: &ConfigPullRequest,
    ) -> Result<Merge, <Self as SettingsEffects>::Error>;

    /// Loads and validates the exact merged commit from the committed source.
    ///
    /// # Errors
    /// Returns an implementation error or committed validation failure.
    fn validate_merged_config(
        &mut self,
        source: &ConfigSource,
        commit: &str,
    ) -> Result<ValidatedConfig, <Self as SettingsEffects>::Error>;

    /// Runs one level-triggered pass using only the validated commit.
    ///
    /// # Errors
    /// Returns an implementation error when the pass cannot start.
    fn reconcile_once(
        &mut self,
        config: &ValidatedConfig,
    ) -> Result<ReconcilePass, <Self as SettingsEffects>::Error>;

    /// Waits for every run started by the pass.
    ///
    /// # Errors
    /// Returns an implementation error when outcome collection fails.
    fn wait_for_outcomes(
        &mut self,
        pass: &ReconcilePass,
    ) -> Result<OutcomeSummary, <Self as SettingsEffects>::Error>;
}
