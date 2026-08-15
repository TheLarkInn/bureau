//! Layer 0: the process contract (DESIGN.md section 7) — the most
//! important interface in the system. Every subprocess in every higher
//! layer goes through [`spawn`].

mod credential;
mod scrub;
mod secret;
mod spawn;

pub use credential::{CredentialError, DIR_VAR, ENV_PREFIX, resolve, resolve_file};
pub use scrub::{REDACTED, ScrubWriter};
pub use secret::Secret;
pub use spawn::{SharedLog, SpawnOutcome, SpawnRequest, SpawnResult, shared_log, spawn};
