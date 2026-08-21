//! Durable pipeline terminal identity.

use serde::{Deserialize, Serialize};

use crate::contract::StepOutcome;

/// The explicit terminal edge that settled a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunTerminal {
    /// Finalize changes and publish when needed.
    Done,
    /// Stop as a failed run.
    Abort,
    /// Stop and request human attention.
    Escalate,
}

/// State projection implied by a terminal run event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalDisposition {
    /// A PR exists for this content.
    Proposed,
    /// The run settled without a PR.
    NoChange,
}

impl TerminalDisposition {
    /// One terminal outcome's durable dedup policy.
    #[must_use]
    pub const fn for_outcome(outcome: StepOutcome, has_pr: bool) -> Option<Self> {
        if has_pr {
            return Some(Self::Proposed);
        }
        match outcome {
            StepOutcome::Failure => None,
            StepOutcome::Success | StepOutcome::Blocked | StepOutcome::NoWork => {
                Some(Self::NoChange)
            }
        }
    }
}
