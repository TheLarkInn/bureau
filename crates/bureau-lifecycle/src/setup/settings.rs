use super::{FlowError, MigrationEffects, PluginEffects, Settings, SettingsEffects};

/// Observable setup state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupState {
    /// Verify that initialization already created settings.
    CheckingSettings,
    /// Optionally install the user-global plugin.
    InstallingPlugin,
    /// Optionally import explicitly selected prior local state.
    MigratingState,
    /// Replace non-secret settings atomically.
    WritingSettings,
    /// Setup finished.
    Complete,
}

/// Existing-installation settings update state machine.
pub struct SetupFlow {
    settings: Settings,
    state: SetupState,
}

impl SetupFlow {
    /// Creates a setup-mode flow.
    #[must_use]
    pub const fn new(settings: Settings) -> Self {
        Self {
            settings,
            state: SetupState::CheckingSettings,
        }
    }

    /// Current state.
    #[must_use]
    pub const fn state(&self) -> SetupState {
        self.state
    }

    /// Advances exactly one state and performs at most one injected effect.
    ///
    /// # Errors
    /// Rejects setup before initialization and propagates effect failures.
    pub fn advance<E>(
        &mut self,
        effects: &mut E,
    ) -> Result<SetupState, FlowError<<E as SettingsEffects>::Error>>
    where
        E: SettingsEffects
            + PluginEffects<Error = <E as SettingsEffects>::Error>
            + MigrationEffects<Error = <E as SettingsEffects>::Error>,
    {
        let next = match self.state {
            SetupState::CheckingSettings => Self::check(effects)?,
            SetupState::InstallingPlugin => self.install(effects)?,
            SetupState::MigratingState => self.migrate(effects)?,
            SetupState::WritingSettings => self.write(effects)?,
            SetupState::Complete => SetupState::Complete,
        };
        self.state = next;
        Ok(next)
    }

    /// Runs every remaining state to completion.
    ///
    /// # Errors
    /// Returns the first state transition failure.
    pub fn run<E>(
        &mut self,
        effects: &mut E,
    ) -> Result<(), FlowError<<E as SettingsEffects>::Error>>
    where
        E: SettingsEffects
            + PluginEffects<Error = <E as SettingsEffects>::Error>
            + MigrationEffects<Error = <E as SettingsEffects>::Error>,
    {
        while self.state != SetupState::Complete {
            self.advance(effects)?;
        }
        Ok(())
    }

    fn check<E: SettingsEffects>(effects: &mut E) -> Result<SetupState, FlowError<E::Error>> {
        if effects.settings_exist().map_err(FlowError::Effect)? {
            Ok(SetupState::InstallingPlugin)
        } else {
            Err(FlowError::SettingsMissing)
        }
    }

    fn write<E: SettingsEffects>(
        &self,
        effects: &mut E,
    ) -> Result<SetupState, FlowError<E::Error>> {
        effects
            .write_settings_atomically(&self.settings)
            .map_err(FlowError::Effect)?;
        Ok(SetupState::Complete)
    }

    fn install<E>(
        &self,
        effects: &mut E,
    ) -> Result<SetupState, FlowError<<E as SettingsEffects>::Error>>
    where
        E: SettingsEffects
            + PluginEffects<Error = <E as SettingsEffects>::Error>
            + MigrationEffects<Error = <E as SettingsEffects>::Error>,
    {
        if self.settings.plugin.install_user_global {
            effects
                .install_user_plugin(&self.settings.plugin)
                .map_err(FlowError::Effect)?;
        }
        Ok(SetupState::MigratingState)
    }

    fn migrate<E>(
        &mut self,
        effects: &mut E,
    ) -> Result<SetupState, FlowError<<E as SettingsEffects>::Error>>
    where
        E: SettingsEffects + MigrationEffects<Error = <E as SettingsEffects>::Error>,
    {
        if self.settings.migration.source.is_some() {
            effects
                .migrate_local_state(&self.settings)
                .map_err(FlowError::Effect)?;
            self.settings.migration.source = None;
        }
        Ok(SetupState::WritingSettings)
    }
}
