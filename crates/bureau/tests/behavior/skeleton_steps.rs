//! Pipeline builders for the walking-skeleton behavior ports: the
//! implement -> apply -> review -> policy -> verdict -> local-ci shape
//! and its rejecting-review variant, as `StepDef` values.

use std::collections::BTreeMap;

use bureau::config::{StepDef, StepKind};

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
        steps: Vec::new(),
        completion: None,
        max_concurrent: None,
        next: None,
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// A deterministic step routing success to `next`, failure to `on_failure`.
pub fn det_step(name: &str, run: &str, next: &str, on_failure: Option<&str>) -> StepDef {
    let mut step = step(name, StepKind::Deterministic);
    step.run = Some(run.to_owned());
    step.next = Some(next.to_owned());
    step.on_failure = on_failure.map(str::to_owned);
    step
}

/// An agent step replaying `fixture`; failures and blocks escalate.
pub fn agent_step(name: &str, role: &str, fixture: &str, next: &str) -> StepDef {
    let mut step = step(name, StepKind::Agent);
    step.role = Some(role.to_owned());
    step.fixture = Some(fixture.to_owned());
    step.next = Some(next.to_owned());
    step.on_failure = Some("escalate".to_owned());
    step.on_blocked = Some("escalate".to_owned());
    step
}

/// A decision routing the watched step's success and failure; blocked
/// escalates and no-work aborts, so the branch map is total.
pub fn decision_step(name: &str, over: &str, success: &str, failure: &str) -> StepDef {
    let mut step = step(name, StepKind::Decision);
    step.over = Some(over.to_owned());
    step.on = BTreeMap::from([
        ("success".to_owned(), success.to_owned()),
        ("failure".to_owned(), failure.to_owned()),
        ("blocked".to_owned(), "escalate".to_owned()),
        ("no-work".to_owned(), "abort".to_owned()),
    ]);
    step
}

/// Routes both of `check`'s outcomes through the decision that watches
/// it — the gate-evaluation shape: the decision, not the step, routes.
fn gated(mut check: StepDef, verdict: &str) -> StepDef {
    check.next = Some(verdict.to_owned());
    check.on_failure = Some(verdict.to_owned());
    check
}

/// The skeleton pipeline: implement (agent) -> apply (deterministic)
/// -> review (agent) -> policy (deterministic verdict input) -> verdict
/// (decision over `policy`) -> local-ci (deterministic) -> done.
///
/// The fake adapter replays a fixed transcript, so the first-pass
/// rejection the scripted reviewer produced in the goober suite is
/// driven here by a marker file in the run directory: `policy` fails
/// the first pass and passes the repass, and `verdict` routes the
/// failure back to `implement` — the needs-changes repass.
pub fn repass_steps(implement: &str, review: &str) -> Vec<StepDef> {
    let flip = "[ -f ../.repassed ] || { touch ../.repassed; exit 1; }";
    let policy = gated(det_step("policy", flip, "", None), "verdict");
    let mut steps = skeleton_steps(implement, review, policy);
    // A routed re-entry consumes attempt budget (goober's skeleton gave
    // implement MaxAttempts=2 for the same reason): the repass runs each
    // of these steps a second time.
    for step in &mut steps {
        step.max_attempts = 2;
    }
    steps.push(decision_step("verdict", "policy", "local-ci", "implement"));
    steps.push(det_step("local-ci", "test -f impl.txt", "done", None));
    steps
}

/// The same shape with a `policy` that always fails: the verdict routes
/// failure to `abort` — the rejecting-review scenario.
pub fn abort_steps(implement: &str, review: &str) -> Vec<StepDef> {
    let policy = gated(det_step("policy", "exit 1", "", None), "verdict");
    let mut steps = skeleton_steps(implement, review, policy);
    steps.push(decision_step("verdict", "policy", "local-ci", "abort"));
    steps.push(det_step("local-ci", "test -f impl.txt", "done", None));
    steps
}

/// The shared head of every skeleton variant.
fn skeleton_steps(implement: &str, review: &str, policy: StepDef) -> Vec<StepDef> {
    vec![
        agent_step("implement", "implementer", implement, "apply"),
        det_step("apply", "echo change >> impl.txt", "review", None),
        agent_step("review", "reviewer", review, "policy"),
        policy,
    ]
}
