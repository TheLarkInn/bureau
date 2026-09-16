#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Api(#[from] crate::forge::github::cloud::Error),
    #[error(transparent)]
    Log(#[from] super::LogError),
    #[error(transparent)]
    Store(#[from] crate::state::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid cloud selection: {0}")]
    Selection(String),
    #[error("unsupported cloud control: {0}")]
    Unsupported(String),
    #[error("cloud request `{0}` is already owned by another local operation")]
    Busy(String),
    #[error(
        "cloud lease ownership was lost or could not be verified; the local operation stopped, but a prepared submission may still execute remotely"
    )]
    LeaseLost,
    #[error(
        "cloud submission `{request_id}` has an uncertain outcome: {message}; inspect this same receipt, do not automatically resubmit"
    )]
    UncertainRecord { request_id: String, message: String },
    #[error("cloud lease release failed: {0}")]
    Release(String),
}

/// Unsupported controls never acquire a token or send a request.
#[must_use]
pub fn unsupported_control() -> Error {
    Error::Unsupported(
        "remote cancellation, pause, resumption, retries, approvals and feedback are unavailable: no authorized Bureau steering contract is established; no request was sent"
            .to_owned(),
    )
}
