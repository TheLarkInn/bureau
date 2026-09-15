//! Direct SDK protocol primitives for opt-in local Copilot factories.

pub mod artifacts;
pub mod context_types;
pub mod definition;
pub mod launch_provider;
pub mod rpc;
pub mod schema;
mod setup_error;
pub mod types;
pub mod usage;
pub mod wire;

pub use setup_error::SetupError;
