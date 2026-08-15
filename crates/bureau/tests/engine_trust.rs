//! Trust-gate regression tests (DESIGN.md section 9): a request's
//! trust is the item's grade lowered to the weakest input's, so
//! agent-derived data can never be laundered past a maintainer gate —
//! not on a fresh run, and not across a resume.

#[path = "engine/rig.rs"]
mod rig;

use bureau::config::StepDef;
use bureau::contract::{StepOutcome, Trust};
use bureau::engine::RunPlan;
use bureau::process::Secret;
use bureau::runlog::{self, EventKind, RunLog};
use rig::{Rig, agent_step, decision_step, det_step, fixture, result};

/// The tests' clock: real millis since the Unix epoch.
fn test_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// draft (agent, `Derived` output) → decide → land, where land gates
/// on `Maintainer` and consumes the draft.
fn gated_steps(rig: &Rig) -> Vec<StepDef> {
    let drafted = fixture(
        rig.dir.path(),
        "draft.json",
        &result(StepOutcome::Success, "drafted"),
    );
    let landed = fixture(
        rig.dir.path(),
        "land.json",
        &result(StepOutcome::Success, "landed"),
    );
    let mut land = agent_step("land", &landed, Some("done"));
    land.inputs_from = vec!["draft".to_owned()];
    land.trust = Some(Trust::Maintainer);
    let mut decide = decision_step("decide", "draft");
    decide.on.insert("success".to_owned(), "land".to_owned());
    vec![agent_step("draft", &drafted, Some("decide")), decide, land]
}

/// The rig's plan with the item re-graded `Maintainer`: a human with
/// write access authored it, so only input provenance can lower trust.
fn maintainer_plan(rig: &Rig, steps: Vec<StepDef>) -> RunPlan {
    let mut plan = rig.plan(steps);
    plan.item.trust = Trust::Maintainer;
    plan
}

/// How often `step` recorded `step_started` in the run's log.
fn starts(rig: &Rig, run_id: &str, step: &str) -> usize {
    let dir = rig.dir.path().join("runs").join(run_id);
    let events = runlog::read_events(&dir).expect("events read");
    events
        .iter()
        .filter(|e| {
            e.kind == EventKind::StepStarted
                && e.data.get("step").and_then(serde_json::Value::as_str) == Some(step)
        })
        .count()
}

/// The gated step never spawned; the escalation comment says why.
fn check_gated_land(rig: &Rig, run_id: &str) {
    assert_eq!(starts(rig, run_id, "land"), 0, "gated step never spawned");
    let comments = rig.forge.comments();
    assert!(
        comments.len() == 1 && comments[0].1.contains("trust"),
        "one escalation comment naming trust: {comments:?}"
    );
}

#[tokio::test]
async fn a_derived_input_fails_a_maintainer_gate() {
    let rig = Rig::new();
    let plan = maintainer_plan(&rig, gated_steps(&rig));
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::Blocked);
    assert_eq!(starts(&rig, &plan.run_id, "draft"), 1);
    check_gated_land(&rig, &plan.run_id);
}

#[tokio::test]
async fn an_inputless_step_keeps_the_item_grade() {
    let rig = Rig::new();
    let transcript = fixture(
        rig.dir.path(),
        "land.json",
        &result(StepOutcome::Success, "ok"),
    );
    let mut land = agent_step("land", &transcript, Some("done"));
    land.trust = Some(Trust::Maintainer);
    let steps = vec![
        det_step("edit", "echo changed >> file.txt", Some("land")),
        land,
    ];
    let outcome = rig.engine().run(&maintainer_plan(&rig, steps)).await;
    assert_eq!(outcome.outcome, StepOutcome::Success);
    assert!(outcome.pr.is_some(), "the gated step ran and opened the PR");
}

#[tokio::test]
async fn a_resumed_run_keeps_the_gate_result() {
    let rig = Rig::new();
    let plan = maintainer_plan(&rig, gated_steps(&rig));
    seed_draft_finished(&rig, &plan);
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::Blocked);
    assert_eq!(starts(&rig, &plan.run_id, "draft"), 1, "draft not re-run");
    check_gated_land(&rig, &plan.run_id);
}

/// A log as if the daemon died right after `draft` succeeded.
fn seed_draft_finished(rig: &Rig, plan: &RunPlan) {
    let secrets = vec![Secret::new("test-credential")];
    let mut log = RunLog::create(
        &rig.dir.path().join("runs"),
        &plan.run_id,
        &secrets,
        test_clock,
    )
    .expect("seeded log creates");
    let started = runlog::run_started_for_item(&plan.run_id, "fix-tests", "42");
    log.append(EventKind::RunStarted, started).expect("append");
    log.append(EventKind::StepStarted, runlog::step_started("draft"))
        .expect("append");
    let finished = runlog::step_finished("draft", StepOutcome::Success);
    log.append(EventKind::StepFinished, finished)
        .expect("append");
    log.close().expect("log closes");
}
