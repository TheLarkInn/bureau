//! Layer 2: the step I/O contract (DESIGN.md section 7).
//!
//! A step receives a [`StepRequest`] as JSON on stdin and answers with a
//! [`StepResult`] as JSON on stdout. Steps communicate only through this
//! contract and through artifact file paths. Both payloads carry a schema
//! version; anything other than [`SCHEMA_VERSION`] is rejected.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The only schema version this build accepts.
pub const SCHEMA_VERSION: &str = "v1";

/// Provenance grade of a step input (DESIGN.md section 9).
///
/// Declaration order defines the ranking, so a step's `min_trust` is a
/// plain `>=` comparison: `Untrusted < Derived < Maintainer < Trusted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// A bot, a build log, or an outside contributor produced it.
    Untrusted,
    /// An agent produced it in an earlier step.
    Derived,
    /// A human with write access wrote it.
    Maintainer,
    /// Checked into the repo on the default branch.
    Trusted,
}

/// What a step concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepOutcome {
    /// The step did what it set out to do.
    Success,
    /// The step failed; the run may retry.
    Failure,
    /// The step cannot proceed; a human must intervene.
    Blocked,
    /// There was nothing for the step to do.
    NoWork,
}

impl StepOutcome {
    /// Whether this outcome consumes retry budget. `Blocked` and `NoWork`
    /// are not failures and never do.
    #[must_use]
    pub const fn consumes_retry(self) -> bool {
        matches!(self, Self::Failure)
    }
}

/// A file produced by a step, recorded in the run log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Short name other steps reference.
    pub name: String,
    /// Path of the artifact file.
    pub path: PathBuf,
}

/// Why a step payload was rejected.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The payload named a schema other than [`SCHEMA_VERSION`].
    #[error("unsupported schema {received:?}; expected {}", SCHEMA_VERSION)]
    Schema {
        /// The received `schema` value.
        received: String,
    },
    /// The payload was not well-formed for its declared schema.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Parses `bytes` as JSON and rejects any schema but [`SCHEMA_VERSION`].
fn checked_value(bytes: &[u8]) -> Result<serde_json::Value, DecodeError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    // `<missing>` is only for an absent field; a present but non-string
    // `schema` renders as its JSON form so the error names what arrived.
    let received = value.get("schema").map_or_else(
        || "<missing>".to_owned(),
        |schema| {
            schema
                .as_str()
                .map_or_else(|| schema.to_string(), str::to_owned)
        },
    );
    if received == SCHEMA_VERSION {
        Ok(value)
    } else {
        Err(DecodeError::Schema { received })
    }
}

/// The input every step receives on stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct StepRequest {
    /// Must equal [`SCHEMA_VERSION`].
    pub schema: String,
    /// Owning run.
    pub run_id: String,
    /// Step name within the pipeline.
    pub step: String,
    /// The run's worktree — the only directory a step may write to.
    pub worktree: PathBuf,
    /// Highest provenance grade of any input.
    pub trust: Trust,
    /// Named inputs carried from earlier steps.
    pub inputs: BTreeMap<String, serde_json::Value>,
    /// Artifacts from earlier steps, by name.
    pub artifacts: BTreeMap<String, PathBuf>,
}

impl StepRequest {
    /// Parses a request, rejecting any schema but [`SCHEMA_VERSION`].
    ///
    /// # Errors
    /// Returns [`DecodeError::Schema`] on a version mismatch and
    /// [`DecodeError::Json`] on malformed JSON.
    pub fn from_json(bytes: &[u8]) -> Result<Self, DecodeError> {
        Ok(serde_json::from_value(checked_value(bytes)?)?)
    }

    /// Serializes to the wire form.
    ///
    /// # Errors
    /// Propagates serialization failures.
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

/// The answer every step writes to stdout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, bon::Builder)]
pub struct StepResult {
    /// Must equal [`SCHEMA_VERSION`].
    pub schema: String,
    /// What the step concluded.
    pub outcome: StepOutcome,
    /// Named outputs for later steps.
    pub outputs: BTreeMap<String, serde_json::Value>,
    /// Artifacts produced by this step.
    pub artifacts: Vec<Artifact>,
    /// Provenance grade of the outputs (usually [`Trust::Derived`]).
    pub trust: Trust,
    /// What this step cost.
    pub cost_usd: f64,
    /// Human-readable detail for the run log.
    pub message: String,
}

impl StepResult {
    /// Parses a result, rejecting any schema but [`SCHEMA_VERSION`].
    ///
    /// # Errors
    /// Returns [`DecodeError::Schema`] on a version mismatch and
    /// [`DecodeError::Json`] on malformed JSON.
    pub fn from_json(bytes: &[u8]) -> Result<Self, DecodeError> {
        Ok(serde_json::from_value(checked_value(bytes)?)?)
    }

    /// Serializes to the wire form.
    ///
    /// # Errors
    /// Propagates serialization failures.
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}
