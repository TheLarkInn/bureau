//! Typed failures before or while attaching approved factory resources.

/// Distinguishes preparation failures without changing their actionable diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    #[error("{0}")]
    Material(String),
    #[error("{0}")]
    Definition(String),
    #[error("{0}")]
    Schema(String),
    #[error("{0}")]
    Arguments(String),
    #[error("{0}")]
    Launch(String),
}

impl From<SetupError> for String {
    fn from(error: SetupError) -> Self {
        error.to_string()
    }
}
