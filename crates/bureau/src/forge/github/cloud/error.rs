use crate::process::{Secret, scrub_json};

fn retry_text(seconds: Option<u64>) -> String {
    seconds.map_or_else(String::new, |value| format!("; retry after {value}s"))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GitHub cloud API rejected the request ({status}){retry}: {message}", retry = retry_text(*retry_after_secs))]
    Api {
        status: u16,
        message: String,
        retry_after_secs: Option<u64>,
    },
    #[error("GitHub cloud transport failed: {0}")]
    Transport(String),
    #[error("invalid GitHub cloud response: {0}")]
    Response(String),
    #[error("GitHub cloud identity mismatch: {0}")]
    Identity(String),
    #[error("incomplete GitHub cloud read: {0}")]
    Incomplete(String),
    #[error("unsupported GitHub cloud operation: {0}")]
    Unsupported(String),
}

impl Error {
    /// A received rejection, rather than an ambiguous send outcome.
    #[must_use]
    pub const fn is_definite_rejection(&self) -> bool {
        matches!(self, Self::Api { status: 400..=499, .. } if !matches!(self, Self::Api { status: 408, .. }))
    }

    pub(super) fn redacted(mut self, secret: &Secret) -> Self {
        let message = match &mut self {
            Self::Api { message, .. }
            | Self::Transport(message)
            | Self::Response(message)
            | Self::Identity(message)
            | Self::Incomplete(message)
            | Self::Unsupported(message) => message,
        };
        let mut value = serde_json::Value::String(std::mem::take(message));
        scrub_json(&mut value, std::slice::from_ref(secret));
        if let serde_json::Value::String(text) = value {
            *message = text;
        }
        self
    }
}
