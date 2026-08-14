//! One agent CLI integration is an adapter (DESIGN.md section 2).
//!
//! Adapters are how a role's agent — an agent file authored in a plugin,
//! referenced unmodified — gets executed. The `fake` adapter replays
//! recorded transcripts, which is what makes every layer above testable
//! offline, deterministically, in CI. Real adapters (`copilot`, `claude`)
//! gain a `record` mode that writes those transcripts.

pub mod fake;

use serde::{Deserialize, Serialize};

/// The agent CLI a role runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdapterKind {
    /// GitHub Copilot CLI.
    Copilot,
    /// Anthropic Claude Code.
    Claude,
    /// Replays a recorded transcript; the test seam for every layer.
    Fake,
}
