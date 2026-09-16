//! Raw, multiplexed JSON-RPC over the supervised process's owned streams.
//!
//! Dropping a call does not retract an accepted command: its response still
//! reaches the handler. Dropping the client aborts both transport tasks; the
//! process supervisor remains responsible for operating-system cleanup.

mod client;
mod dispatch;
mod envelope;
mod error;
mod handler;
mod reader;
mod shutdown;

pub use client::Client;
pub use error::Error;
pub use handler::{CallbackRequest, Handler};

/// The handler API's remote fault type, distinct from connection failures.
pub type RpcFault = super::types::RpcError;

#[cfg(test)]
mod tests;
