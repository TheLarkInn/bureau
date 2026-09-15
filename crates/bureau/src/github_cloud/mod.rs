//! Explicit cloud submissions and observations, not a pipeline engine.

mod control;
mod dispatch;
mod error;
mod monitor;
mod record_log;
mod records;
mod selection;

pub use control::Control;
pub use dispatch::dispatch;
pub use error::{Error, unsupported_control};
pub use monitor::{refresh, track};
pub use record_log::{LEASE_ASSIGNMENT, Log, lease_key, read_state, validate_key};
pub type LogError = record_log::Error;
pub use records::{Dispatch, Record, Scope, Start, State};
pub use selection::{ExpectedIdentity, Selection, SelectionRequest, select};
