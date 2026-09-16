//! Durable local-runtime facts, separate from cloud factory task identities.

mod model;
mod replay;
mod transitions;

use std::num::NonZeroU64;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::copilot_factory::artifacts::{Identity, Paths};
use crate::adapters::copilot_factory::context_types::PinnedContext;
use crate::adapters::copilot_factory::types::{FactoryRunResult, FactoryRunSummary};
use crate::config::CopilotFactory;
use crate::contract::StepRequest;

pub use model::{Record, Records};

/// Original filesystem identity: a recreated path is not the journal's workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    /// Canonical owning worktree.
    pub directory: PathBuf,
    /// Original filesystem device.
    pub device: u64,
    /// Original directory inode.
    pub inode: u64,
    /// Original canonical mirror, never refreshed before factory recovery.
    pub mirror: PathBuf,
    /// Exact run branch.
    pub branch: String,
    /// Base used for the ordinary final diff.
    pub start_head: String,
}

/// Authority and immutable identities persisted before runtime initialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    /// Pipeline step, not a runtime factory name.
    pub step: String,
    /// Bureau step attempt whose execution owns this session.
    pub step_attempt: u32,
    /// Caller-selected SDK session ID, never a cloud task ID.
    pub session_id: String,
    /// Original workspace identity.
    pub workspace: Workspace,
    /// Private runtime storage and executable locations.
    pub paths: Paths,
    /// Original project source versus actual session-provider identity.
    pub identity: Identity,
    /// Reviewed invocation configuration and exact static args.
    pub factory: CopilotFactory,
    /// Immutable step inputs, distinct from the factory's static arguments.
    pub request: StepRequest,
    /// Approved native context; restore verifies only these private pins.
    pub context: PinnedContext,
}

/// An explicitly authorized runtime control request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// Fresh admission; has no caller idempotency key.
    Start,
    /// Continue one already-known runtime run without new ceilings.
    Resume,
    /// Orderly resumable stop.
    Pause,
    /// Non-resumable cancellation.
    Cancel,
}

/// Inspection cannot retroactively prove the original execution process exited cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePurpose {
    /// Read the already-known durable runtime identity without running a factory.
    Inspection,
    /// Initialize the approved provider and execute or resume its factory.
    Execution,
}

/// One local factory event, always written through the checked ownership fence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Data {
    /// Persisted before an executable or extension is started.
    Prepared {
        /// Complete approved identity.
        intent: Box<Intent>,
    },
    /// The SDK acknowledged the exact preselected session ID.
    SessionAccepted {
        /// Runtime session.
        session_id: String,
    },
    /// Persisted before starting a new supervised runtime process.
    RuntimeOpened {
        /// Owning durable session.
        session_id: String,
        /// Whether this process may execute a factory.
        purpose: RuntimePurpose,
    },
    /// Native control intent persisted before sending the request.
    Dispatch {
        /// Runtime session.
        session_id: String,
        /// Requested operation.
        operation: Operation,
    },
    /// A correlated raw RPC reply acknowledged the runtime run ID and attempt.
    Accepted {
        /// Runtime session.
        session_id: String,
        /// Server-generated run identity.
        run_id: String,
        /// Actual native attempt.
        attempt: NonZeroU64,
    },
    /// An uncorrelated lifecycle notification; never implicit start acceptance.
    Notification {
        /// Owning SDK session.
        session_id: String,
        /// Exact lifecycle event after secret scrubbing.
        notification: Value,
    },
    /// Authoritative raw result and complete accounting projection.
    Observed {
        /// Runtime session.
        session_id: String,
        /// Full run envelope, including actual result rather than preview.
        run: Box<FactoryRunResult>,
        /// Identity, resumability, and cumulative accounting from detail.
        summary: Box<FactoryRunSummary>,
    },
    /// The runtime positively rejected admission before creating a run.
    Rejected {
        /// Runtime session.
        session_id: String,
        /// Typed RPC failure diagnosis.
        message: String,
    },
    /// Shutdown acknowledgement and actual process/descendant exit were checked.
    RuntimeClosed {
        /// Runtime session.
        session_id: String,
        /// Inspection shutdown is not execution shutdown.
        purpose: RuntimePurpose,
        /// Both acknowledgement and clean supervised exit succeeded.
        clean: bool,
        /// Failure diagnosis when shutdown was not clean.
        message: String,
    },
    /// Human intervention is required; no retry or orphan-name matching is allowed.
    Indeterminate {
        /// Runtime session.
        session_id: String,
        /// Actionable diagnosis.
        message: String,
    },
}

impl Data {
    /// The exact owning runtime session for every event.
    #[must_use]
    pub fn session_id(&self) -> &str {
        match self {
            Self::Prepared { intent } => &intent.session_id,
            Self::SessionAccepted { session_id }
            | Self::RuntimeOpened { session_id, .. }
            | Self::Dispatch { session_id, .. }
            | Self::Accepted { session_id, .. }
            | Self::Notification { session_id, .. }
            | Self::Observed { session_id, .. }
            | Self::Rejected { session_id, .. }
            | Self::RuntimeClosed { session_id, .. }
            | Self::Indeterminate { session_id, .. } => session_id,
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Prepared { intent } => {
                format!("{}: prepared {}", intent.step, intent.factory.name)
            }
            Self::SessionAccepted { session_id } => format!("SDK session accepted {session_id}"),
            Self::RuntimeOpened { purpose, .. } => format!("SDK runtime {purpose:?} opened"),
            Self::Dispatch { operation, .. } => format!("factory {operation:?} requested"),
            Self::Accepted {
                run_id, attempt, ..
            } => format!("factory {run_id} attempt {attempt}"),
            Self::Observed { run, .. } => format!("factory {} {:?}", run.run_id, run.status),
            Self::Notification { .. } => "factory lifecycle notification".to_owned(),
            Self::Rejected { message, .. }
            | Self::RuntimeClosed { message, .. }
            | Self::Indeterminate { message, .. } => message.clone(),
        }
    }
}

/// Renders malformed local-factory events explicitly, never as an empty success.
#[must_use]
pub fn gist(value: &Value) -> String {
    match serde_json::from_value::<Data>(value.clone()) {
        Ok(data) => data.summary(),
        Err(error) => format!("invalid local factory event: {error}"),
    }
}
