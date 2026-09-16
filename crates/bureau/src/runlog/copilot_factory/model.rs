//! Passive native lifecycle state, shared by validation and replay.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

use super::{Intent, Operation, RuntimePurpose};
use crate::adapters::copilot_factory::types::{
    FactoryRunConsumed, FactoryRunResult, FactoryRunSummary,
};

/// One execution identity and its last authoritative observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Original authorization and code/context identities.
    pub intent: Intent,
    /// The runtime acknowledged the exact session ID.
    pub session_accepted: bool,
    /// Latest explicitly requested control operation.
    pub dispatched: Option<Operation>,
    /// Correlated admission identity; never populated from notifications.
    pub run_id: Option<String>,
    /// Correlated native attempt.
    pub attempt: Option<NonZeroU64>,
    /// Actual raw run result, not a detail preview.
    pub run: Option<FactoryRunResult>,
    /// Required accounting and resume eligibility from detail.
    pub summary: Option<FactoryRunSummary>,
    /// Per-run cumulative high-water marks, never summed snapshots.
    pub consumed: Option<FactoryRunConsumed>,
    /// At least one execution process may have started.
    pub execution_opened: bool,
    /// Last execution process had acknowledged, clean supervised shutdown.
    pub execution_clean: bool,
    /// Positive pre-admission rejection, when present.
    pub rejected: Option<String>,
    /// Explicitly indeterminate state; inspection must not clear it by inference.
    pub indeterminate: Option<String>,
    /// An opened process with no recorded close. A crash leaves this set.
    pub active_runtime: Option<RuntimePurpose>,
    /// The runtime purpose whose owning SDK session was acknowledged.
    pub runtime_session: Option<RuntimePurpose>,
    /// A run/resume dispatch waiting for its correlated raw response.
    pub pending_admission: Option<Operation>,
}

impl From<Intent> for Record {
    fn from(intent: Intent) -> Self {
        Self {
            intent,
            session_accepted: false,
            dispatched: None,
            run_id: None,
            attempt: None,
            run: None,
            summary: None,
            consumed: None,
            execution_opened: false,
            execution_clean: false,
            rejected: None,
            indeterminate: None,
            active_runtime: None,
            runtime_session: None,
            pending_admission: None,
        }
    }
}

/// Strictly replayed local factory identities, keyed by the caller-chosen session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Records(pub BTreeMap<String, Record>);
