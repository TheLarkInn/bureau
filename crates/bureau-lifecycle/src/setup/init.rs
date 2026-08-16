mod data;

use super::{
    ConfigDraft, ConfigPullRequest, FlowError, InitEffects, InitOutcome, InitRequest, Merge,
    OutcomeSummary, ReconcilePass, SettingsEffects, ValidatedConfig,
};

type EffectError<E> = <E as SettingsEffects>::Error;

/// Observable initialization state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitState {
    /// Verify that initialization has not already completed.
    CheckingSettings,
    /// Select bundled files or invoke the injected authoring effect.
    PreparingConfig,
    /// Present the complete draft.
    PreviewingConfig,
    /// Validate the presented bytes without executing them.
    ValidatingPreview,
    /// Propose the validated draft.
    CreatingPullRequest,
    /// Wait for review and merge.
    WaitingForMerge,
    /// Validate the exact merged commit.
    ValidatingMergedCommit,
    /// Persist local non-secret settings atomically.
    WritingSettings,
    /// Optionally install the user-global plugin.
    InstallingPlugin,
    /// Run one pass against the validated commit.
    Reconciling,
    /// Wait for runs started by the pass.
    WaitingForOutcomes,
    /// Initialization finished.
    Complete,
}

/// First-time initialization state machine.
pub struct InitFlow {
    request: InitRequest,
    state: InitState,
    draft: Option<ConfigDraft>,
    pull_request: Option<ConfigPullRequest>,
    merged: Option<Merge>,
    validated: Option<ValidatedConfig>,
    pass: Option<ReconcilePass>,
    outcome: Option<InitOutcome>,
}

impl InitFlow {
    /// Creates a first-time flow.
    #[must_use]
    pub const fn new(request: InitRequest) -> Self {
        Self {
            request,
            state: InitState::CheckingSettings,
            draft: None,
            pull_request: None,
            merged: None,
            validated: None,
            pass: None,
            outcome: None,
        }
    }

    /// Current state.
    #[must_use]
    pub const fn state(&self) -> InitState {
        self.state
    }

    /// Advances exactly one state and performs at most one injected effect.
    ///
    /// # Errors
    /// Rejects repeated initialization, effect failures, and merged-commit drift.
    pub fn advance<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        let next = self.next(effects)?;
        self.state = next;
        Ok(next)
    }

    /// Runs every remaining state to completion.
    ///
    /// # Errors
    /// Returns the first state transition failure.
    pub fn run<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitOutcome, FlowError<EffectError<E>>> {
        while self.state != InitState::Complete {
            self.advance(effects)?;
        }
        self.outcome
            .clone()
            .ok_or(FlowError::MissingStateData("init outcome"))
    }

    fn next<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        match self.state {
            InitState::CheckingSettings => Self::check_settings(effects),
            InitState::PreparingConfig => self.prepare_config(effects),
            InitState::PreviewingConfig => self.preview(effects),
            InitState::ValidatingPreview => self.validate_preview(effects),
            InitState::CreatingPullRequest => self.create_pull_request(effects),
            InitState::WaitingForMerge => self.wait_for_merge(effects),
            InitState::ValidatingMergedCommit => self.validate_merged(effects),
            InitState::WritingSettings => self.write_settings(effects),
            InitState::InstallingPlugin => self.install_plugin(effects),
            InitState::Reconciling => self.reconcile(effects),
            InitState::WaitingForOutcomes => self.wait_for_outcomes(effects),
            InitState::Complete => Ok(InitState::Complete),
        }
    }

    fn check_settings<E: InitEffects>(
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        if effects.settings_exist().map_err(FlowError::Effect)? {
            Err(FlowError::SettingsAlreadyExist)
        } else {
            Ok(InitState::InstallingPlugin)
        }
    }

    fn prepare_config<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        self.draft = Some(
            effects
                .prepare_config(&self.request.first_pipeline)
                .map_err(FlowError::Effect)?,
        );
        Ok(InitState::PreviewingConfig)
    }

    fn preview<E: InitEffects>(
        &self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        effects
            .preview_config(&self.request.settings.config, self.draft()?)
            .map_err(FlowError::Effect)?;
        Ok(InitState::ValidatingPreview)
    }

    fn validate_preview<E: InitEffects>(
        &self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        effects
            .validate_config_preview(self.draft()?)
            .map_err(FlowError::Effect)?;
        Ok(InitState::CreatingPullRequest)
    }

    fn create_pull_request<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        let pull_request = effects
            .create_config_pull_request(&self.request.settings.config, self.draft()?)
            .map_err(FlowError::Effect)?;
        self.pull_request = Some(pull_request);
        Ok(InitState::WaitingForMerge)
    }

    fn wait_for_merge<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        let merged = effects
            .wait_for_merge(self.pull_request()?)
            .map_err(FlowError::Effect)?;
        self.merged = Some(merged);
        Ok(InitState::ValidatingMergedCommit)
    }

    fn validate_merged<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        let merged = self.merged()?.commit.clone();
        let source = &self.request.settings.config;
        let validated = effects
            .validate_merged_config(source, &merged)
            .map_err(FlowError::Effect)?;
        self.verify_validated(&merged, &validated)?;
        self.validated = Some(validated);
        Ok(InitState::Reconciling)
    }

    fn write_settings<E: InitEffects>(
        &self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        effects
            .write_settings_atomically(&self.request.settings)
            .map_err(FlowError::Effect)?;
        Ok(InitState::Complete)
    }

    fn install_plugin<E: InitEffects>(
        &self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        if self.request.settings.plugin.install_user_global {
            effects
                .install_user_plugin(&self.request.settings.plugin)
                .map_err(FlowError::Effect)?;
        }
        Ok(InitState::PreparingConfig)
    }

    fn reconcile<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        let pass = effects
            .reconcile_once(self.validated()?)
            .map_err(FlowError::Effect)?;
        self.pass = Some(pass);
        Ok(InitState::WaitingForOutcomes)
    }

    fn wait_for_outcomes<E: InitEffects>(
        &mut self,
        effects: &mut E,
    ) -> Result<InitState, FlowError<EffectError<E>>> {
        let outcomes = effects
            .wait_for_outcomes(self.pass()?)
            .map_err(FlowError::Effect)?;
        self.finish(outcomes)?;
        Ok(InitState::WritingSettings)
    }
}
