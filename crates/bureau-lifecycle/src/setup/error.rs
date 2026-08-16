/// Local lifecycle flow failure.
#[derive(Debug, thiserror::Error)]
pub enum FlowError<E> {
    /// An injected operation failed.
    #[error(transparent)]
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
