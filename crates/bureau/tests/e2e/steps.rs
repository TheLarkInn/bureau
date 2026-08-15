//! The reference pipeline as `StepDef` values (DESIGN.md section 11)
//! and the fake-adapter fixtures its agent steps replay.

use std::collections::BTreeMap;
use std::path::Path;

use bureau::adapters::fake::{Chunk, Stream, Transcript};
use bureau::config::{StepDef, StepKind};
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};

/// The failing check the scenario is built around.
pub const CHECK: &str = "grep -q 42 answer.txt";

/// The fix and its re-check: apply the patch, re-run the test.
const APPLY: &str = "printf '42\\n' > answer.txt && grep -q 42 answer.txt";

/// A fake-adapter fixture whose stdout is the step result.
pub fn fixture(dir: &Path, name: &str, outcome: StepOutcome, message: &str) -> String {
    let result = StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    };
    let mut data = String::from_utf8(result.to_json().expect("result serializes")).expect("utf8");
    data.push('\n');
    let transcript = Transcript {
        schema: SCHEMA_VERSION.to_owned(),
        chunks: vec![Chunk {
            delay_ms: 0,
            stream: Stream::Stdout,
            data,
        }],
        exit_code: 0,
        usage: bureau::adapters::Usage::zero("fake"),
    };
    let path = dir.join(name);
    transcript.save(&path).expect("fixture saves");
    path.to_string_lossy().into_owned()
}

/// A step with every optional field unset.
fn step(name: &str, kind: StepKind) -> StepDef {
    StepDef {
        name: name.to_owned(),
        kind,
        run: None,
        role: None,
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        next: None,
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// A deterministic step running `run` with the given success edge.
fn det(name: &str, run: &str, next: &str, on_failure: Option<&str>) -> StepDef {
    let mut step = step(name, StepKind::Deterministic);
    step.run = Some(run.to_owned());
    step.next = Some(next.to_owned());
    step.on_failure = on_failure.map(str::to_owned);
    step
}

/// An agent step replaying `fixture`; failures escalate.
fn agent(name: &str, role: &str, fixture: &str, next: &str) -> StepDef {
    let mut step = step(name, StepKind::Agent);
    step.role = Some(role.to_owned());
    step.fixture = Some(fixture.to_owned());
    step.next = Some(next.to_owned());
    step.on_failure = Some("escalate".to_owned());
    step.on_blocked = Some("escalate".to_owned());
    step
}

/// A decision routing failure back to `propose`, success onward.
fn decision(name: &str, over: &str, success: &str) -> StepDef {
    let mut step = step(name, StepKind::Decision);
    step.over = Some(over.to_owned());
    step.on = BTreeMap::from([
        ("success".to_owned(), success.to_owned()),
        ("failure".to_owned(), "propose".to_owned()),
        ("blocked".to_owned(), "escalate".to_owned()),
        ("no-work".to_owned(), "abort".to_owned()),
    ]);
    step
}

/// The reference pipeline (DESIGN.md section 11): reproduce fails, the
/// agent proposes, `apply` writes the fix and re-checks, review and
/// verdict check it, verify re-runs the check. Push and PR live in the
/// engine's `done` terminal, so no publish step is needed.
pub fn reference_steps(propose: &str, review: &str, max_attempts: u32) -> Vec<StepDef> {
    let mut propose_step = agent("propose", "implementer", propose, "apply");
    propose_step.inputs_from = vec!["reproduce".to_owned()];
    propose_step.max_attempts = max_attempts;
    let mut review_step = agent("review", "reviewer", review, "verdict");
    review_step.inputs_from = vec!["apply".to_owned()];
    vec![
        det("claim", "true", "reproduce", None),
        det("reproduce", CHECK, "done", Some("propose")),
        propose_step,
        det("apply", APPLY, "passed", Some("escalate")),
        decision("passed", "apply", "review"),
        review_step,
        decision("verdict", "review", "verify"),
        det("verify", CHECK, "done", Some("propose")),
    ]
}
