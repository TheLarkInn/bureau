/// Local lifecycle flow failure.
#[derive(thiserror::Error)]
pub enum FlowError<E> {
    /// An injected operation failed.
    #[error("an injected operation failed")]
    Effect(E),
    /// `init` was attempted after local settings had been created.
    #[error("local settings already exist; use `bureau setup` to change them")]
    SettingsAlreadyExist,
    /// `setup` was attempted before initialization.
    #[error("local settings do not exist; use `bureau init` first")]
    SettingsMissing,
    /// The committed validator returned a different source.
    #[error("validated config source differs from the configured committed source")]
    ValidatedSourceMismatch,
    /// The committed validator returned a different revision.
    #[error("merged commit `{merged}` was not the validated commit `{validated}`")]
    MergedCommitMismatch {
        /// Commit returned by the merge wait.
        merged: String,
        /// Commit returned by committed config validation.
        validated: String,
    },
    /// A private state invariant was not satisfied.
    #[error("init state is missing {0}")]
    MissingStateData(&'static str),
}

// Hand-written so the impl holds for every `E`; callers unwrap `Effect` to
// recover the typed effect failure, so rendering it here adds nothing.
impl<E> std::fmt::Debug for FlowError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let variant = match self {
            Self::Effect(_) => "Effect",
            Self::SettingsAlreadyExist => "SettingsAlreadyExist",
            Self::SettingsMissing => "SettingsMissing",
            Self::ValidatedSourceMismatch => "ValidatedSourceMismatch",
            Self::MergedCommitMismatch { .. } => "MergedCommitMismatch",
            Self::MissingStateData(_) => "MissingStateData",
        };
        formatter.write_str(variant)
    }
}
