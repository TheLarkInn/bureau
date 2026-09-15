//! Versioned submission facts and explicitly selected remote observations.

mod transition;
mod validate;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runlog::ConfigSource;

/// The reviewed repository and verified principal bound to a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Canonical GitHub owner/repository name, not a URL.
    pub repo: String,
    /// Name of the committed repository registry entry.
    pub registry_name: String,
    /// Credential reference; never the credential value.
    pub credential_reference: String,
    /// Verified stable GitHub user identity.
    pub principal_id: u64,
    /// Login observed when the principal was verified.
    pub principal_login: String,
    /// Exact committed configuration identity.
    #[serde(deserialize_with = "validate::config_source")]
    pub config_source: ConfigSource,
}

/// Immutable identity and definition captured before any submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    /// Caller-selected local request key.
    pub request_id: String,
    /// Reviewed repository and verified principal.
    pub scope: Scope,
    /// Exact existing automation identity.
    pub automation_id: String,
    /// Inspected definition, preserved as data.
    pub definition: Value,
}

/// Submission delivery evidence, never a remote execution outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dispatch {
    /// No send intent has been recorded.
    NotSubmitted,
    /// The submission may have been sent; interrupted reads are uncertain.
    Prepared,
    /// The service accepted the submission, without returning a task identity.
    Accepted,
    /// The service definitively rejected the submission.
    Rejected,
    /// Delivery or the response could not be established.
    Uncertain,
}

impl Dispatch {
    /// Stable wire label. `Prepared` must not be presented as safe to resend.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotSubmitted => "not_submitted",
            Self::Prepared => "prepared",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Uncertain => "uncertain",
        }
    }
}

/// A proposed durable change. No variant sends or controls remote work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    /// Persisted immediately before an eligible explicit submission.
    Prepared {
        /// Exact supported dispatch event: `manual` or `interval`.
        event: String,
    },
    /// Acceptance only; there is no inferred task identity.
    Accepted,
    /// A definite rejection.
    Rejected {
        /// Scrubbed at the log write boundary.
        message: String,
    },
    /// Ambiguous submission delivery.
    Uncertain {
        /// Scrubbed at the log write boundary.
        message: String,
    },
    /// An exact task selected by the caller, not correlated by the service.
    TaskSelected {
        /// Opaque remote task identity, not a local path component.
        task_id: String,
    },
    /// An authoritative read of that exact task.
    Observed {
        /// Raw task object, including open-ended state and status values.
        task: Value,
        /// Newly read event objects; omission retains older event evidence.
        events: Option<Vec<Value>>,
        /// Server-reported event total, not a snapshot completeness guarantee.
        reported_total: Option<u32>,
    },
}

/// Receipt state derived solely from durable cloud events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct State {
    /// Immutable scope and selected automation.
    pub start: Start,
    /// Submission delivery evidence only.
    pub dispatch: Dispatch,
    /// Latest delivery explanation, when supplied.
    pub dispatch_message: Option<String>,
    /// Dispatch event recorded before submission.
    pub event: Option<String>,
    /// Exact caller-selected task, independent of submission acceptance.
    pub task_id: Option<String>,
    /// Always `operator_selected_unproven` when a task has been selected.
    pub task_correlation: Option<&'static str>,
    /// Last durably observed task object.
    pub task: Option<Value>,
    /// Last durably observed event objects.
    pub events: Vec<Value>,
    /// Last event read's reported total, absent when that read supplied none.
    pub events_reported_total: Option<u32>,
    /// Outer log timestamp of the last task observation.
    pub observed_at_ms: Option<u64>,
    /// Outer timestamp when these events, not merely the task, were read.
    pub events_observed_at_ms: Option<u64>,
}

impl State {
    pub(super) fn apply(&mut self, record: &Record, at_ms: u64) -> Result<(), &'static str> {
        transition::apply(self, record, at_ms)
    }
}

pub(super) fn created(start: Start) -> Result<State, &'static str> {
    validate::start(&start)?;
    Ok(State {
        start,
        dispatch: Dispatch::NotSubmitted,
        dispatch_message: None,
        event: None,
        task_id: None,
        task_correlation: None,
        task: None,
        events: Vec::new(),
        events_reported_total: None,
        observed_at_ms: None,
        events_observed_at_ms: None,
    })
}
