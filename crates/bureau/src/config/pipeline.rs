//! `pipelines/<name>.yaml` — the step state machine (DESIGN.md layer 4).
//!
//! Exactly two step types run code: `deterministic` (a shell command via
//! the layer-0 contract) and `agent` (an adapter). A `decision` step
//! names one edge per outcome and never runs code. Edges are explicit; a
//! missing branch fails closed — validation rejects an incomplete
//! decision, and a step without an edge for an outcome aborts.
//!
//! Edge targets are step names or a terminal: `done` (finalize: push the
//! branch and open a PR), `abort` (stop; failure), `escalate` (stop;
//! comment for a human). `join` is reserved and rejected in v0.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::files::Named;
use crate::contract::Trust;

/// Terminal edge targets.
pub const TERMINALS: [&str; 4] = ["done", "abort", "escalate", "join"];

/// A pipeline definition: the file you edit is the file that runs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    /// Must match the file stem.
    pub name: String,
    /// The steps, in declaration order. The first step is the entry.
    pub steps: Vec<StepDef>,
}

impl Named for Pipeline {
    fn name(&self) -> &str {
        &self.name
    }
}

/// The step's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    /// Runs code: a shell command through the layer-0 contract.
    Deterministic,
    /// Runs an adapter (an agent CLI).
    Agent,
    /// Branches on a previous step's outcome; runs no code.
    Decision,
}

/// One step. Which fields apply depends on `kind`:
///
/// - `deterministic`: `run` required.
/// - `agent`: `role` required; `fixture` only with the `fake` adapter;
///   `trust` overrides the role's `min_trust`.
/// - `decision`: `over` and a complete `on` (all four outcomes) required.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    /// Step name, unique within the pipeline.
    pub name: String,
    /// What kind of step this is.
    #[serde(rename = "type")]
    pub kind: StepKind,
    /// Shell command (`deterministic`).
    #[serde(default)]
    pub run: Option<String>,
    /// Role name (`agent`).
    #[serde(default)]
    pub role: Option<String>,
    /// Transcript fixture path (`agent` with the `fake` adapter). Must be
    /// absolute: it is a testing seam, not config-portable state.
    #[serde(default)]
    pub fixture: Option<String>,
    /// Minimum input trust this step accepts; defaults to the role's
    /// `min_trust`.
    #[serde(default)]
    pub trust: Option<Trust>,
    /// The step whose outcome a `decision` branches on.
    #[serde(default)]
    pub over: Option<String>,
    /// Outcome-to-target edges for a `decision`; must cover exactly
    /// `success`, `failure`, `blocked`, `no-work`.
    #[serde(default)]
    pub on: BTreeMap<String, String>,
    /// Successor on success. Absent means `abort` (fail closed).
    #[serde(default)]
    pub next: Option<String>,
    /// Edge on failure. Absent means `abort`.
    #[serde(default)]
    pub on_failure: Option<String>,
    /// Edge on blocked. Absent means `abort`.
    #[serde(default)]
    pub on_blocked: Option<String>,
    /// Edge on no-work. Absent means `abort`.
    #[serde(default)]
    pub on_no_work: Option<String>,
    /// Earlier steps whose outputs and artifacts flow into this step.
    #[serde(default)]
    pub inputs_from: Vec<String>,
    /// Times this step may be entered before the run escalates.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Per-step spawn timeout.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

const fn default_max_attempts() -> u32 {
    1
}
