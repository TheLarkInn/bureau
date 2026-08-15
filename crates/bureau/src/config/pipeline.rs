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

impl StepKind {
    /// The lowercase name used in config files and error messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Agent => "agent",
            Self::Decision => "decision",
        }
    }
}
const fn default_max_attempts() -> u32 {
    1
}
/// The four outcomes a `decision` step's `on` must cover (kebab-case).
const OUTCOMES: [&str; 4] = ["success", "failure", "blocked", "no-work"];
fn allowed_on(kind: StepKind, field: &str) -> bool {
    match kind {
        StepKind::Deterministic => field == "run",
        StepKind::Agent => matches!(field, "role" | "fixture" | "trust"),
        StepKind::Decision => matches!(field, "over" | "on"),
    }
}
fn check_missing_outcomes(on: &BTreeMap<String, String>, errors: &mut Vec<String>) {
    for outcome in OUTCOMES {
        if !on.contains_key(outcome) {
            errors.push(format!("`on` is missing a `{outcome}` branch"));
        }
    }
}
fn check_unknown_outcomes(on: &BTreeMap<String, String>, errors: &mut Vec<String>) {
    for key in on.keys() {
        if !OUTCOMES.contains(&key.as_str()) {
            errors.push(format!("`on` has unknown outcome `{key}`"));
        }
    }
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
impl StepDef {
    /// Every edge target this step names.
    pub fn edge_targets(&self) -> impl Iterator<Item = &str> {
        [
            self.next.as_deref(),
            self.on_failure.as_deref(),
            self.on_blocked.as_deref(),
            self.on_no_work.as_deref(),
        ]
        .into_iter()
        .flatten()
        .chain(self.on.values().map(String::as_str))
    }

    /// Field-level problems with this step, independent of other config.
    #[must_use]
    pub fn field_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        self.check_required(&mut errors);
        self.check_misplaced(&mut errors);
        self.check_limits(&mut errors);
        self.check_coverage(&mut errors);
        errors
    }

    fn check_required(&self, errors: &mut Vec<String>) {
        match self.kind {
            StepKind::Deterministic => self.check_run(errors),
            StepKind::Agent => self.check_role(errors),
            StepKind::Decision => self.check_over(errors),
        }
    }

    fn check_run(&self, errors: &mut Vec<String>) {
        if self.run.as_deref().is_none_or(|r| r.trim().is_empty()) {
            errors.push("`run` is required for a deterministic step".to_owned());
        }
    }

    fn check_role(&self, errors: &mut Vec<String>) {
        if self.role.is_none() {
            errors.push("`role` is required for an agent step".to_owned());
        }
    }

    fn check_over(&self, errors: &mut Vec<String>) {
        if self.over.is_none() {
            errors.push("`over` is required for a decision step".to_owned());
        }
    }

    fn check_misplaced(&self, errors: &mut Vec<String>) {
        let fields = [
            ("run", self.run.is_some()),
            ("role", self.role.is_some()),
            ("fixture", self.fixture.is_some()),
            ("trust", self.trust.is_some()),
            ("over", self.over.is_some()),
            ("on", !self.on.is_empty()),
        ];
        for (field, present) in fields {
            if present && !allowed_on(self.kind, field) {
                errors.push(format!(
                    "`{field}` does not apply to {} steps",
                    self.kind.name()
                ));
            }
        }
    }

    fn check_limits(&self, errors: &mut Vec<String>) {
        if self.max_attempts == 0 {
            errors.push("`max_attempts` must be at least 1".to_owned());
        }
        if self.timeout_secs == Some(0) {
            errors.push("`timeout_secs` must be positive".to_owned());
        }
    }

    fn check_coverage(&self, errors: &mut Vec<String>) {
        if self.kind == StepKind::Decision {
            check_missing_outcomes(&self.on, errors);
            check_unknown_outcomes(&self.on, errors);
        }
    }
}
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
