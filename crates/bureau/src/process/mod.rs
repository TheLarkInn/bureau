//! Layer 0: the process contract (DESIGN.md section 7) — the most
//! important interface in the system. Every subprocess in every higher
//! layer goes through [`spawn`] or its supervised [`duplex`] variant.

use serde::{Deserialize, Serialize};

mod credential;
mod duplex;
mod scrub;
mod secret;
mod spawn;
mod utf8;
mod wait;

pub use credential::{CredentialError, DIR_VAR, ENV_PREFIX, resolve, resolve_file};
pub use duplex::{Duplex, DuplexOwner, duplex, start_duplex};
pub use scrub::{REDACTED, ScrubWriter, scrub_json};
pub use secret::Secret;
pub use spawn::{SharedLog, SpawnRequest, SpawnResult, shared_log, spawn};
pub(crate) use utf8::complete_prefix;

/// How a spawned process ended.
///
/// These four outcomes are genuinely distinct; a timeout is never collapsed
/// into an exit code. Defined in the parent module so `spawn` and `wait` can
/// share it without a sibling cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnOutcome {
    /// Ran to completion; see `exit_code`.
    Exited,
    /// Hard-killed (the whole process group) after `timeout`.
    Timeout,
    /// Killed externally by a signal.
    Signaled,
    /// Never started.
    SpawnFailed,
}
