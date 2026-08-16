//! MCP step context and one-shot result publication.
//!
//! Adapters create a [`Session`] and merge its environment into the child
//! process. The hidden CLI entrypoint serves the two MCP tools over stdio.

mod paths;
mod protocol;
mod server;
mod session;
mod tools;

pub use paths::Paths;
pub use server::{serve, serve_stdio};
pub use session::Session;

/// Environment variable containing the immutable step request path.
pub const BUREAU_STEP_REQUEST: &str = "BUREAU_STEP_REQUEST";
/// Environment variable containing the one-shot step result path.
pub const BUREAU_STEP_RESULT: &str = "BUREAU_STEP_RESULT";

const REQUEST_FILE: &str = "request.json";
const RESULT_FILE: &str = "result.json";
