use std::io;
use std::sync::Arc;

use super::RpcFault;

/// Remote failures do not close the connection; all other variants are terminal.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Remote(RpcFault),
    #[error("JSON-RPC transport failed: {0}")]
    Transport(#[source] Arc<io::Error>),
    #[error("invalid JSON-RPC message: {0}")]
    Protocol(String),
    #[error("JSON-RPC {callback} observer failed for {method}: {source}")]
    Observer {
        callback: &'static str,
        method: String,
        #[source]
        source: Arc<io::Error>,
    },
    #[error("JSON-RPC request ID space exhausted")]
    IdExhausted,
    #[error("JSON-RPC connection closed: {0}")]
    Closed(&'static str),
}

impl Error {
    pub(super) fn transport(source: io::Error) -> Self {
        Self::Transport(Arc::new(source))
    }

    pub(super) fn observer(callback: &'static str, method: &str, source: io::Error) -> Self {
        Self::Observer {
            callback,
            method: method.to_owned(),
            source: Arc::new(source),
        }
    }
}
