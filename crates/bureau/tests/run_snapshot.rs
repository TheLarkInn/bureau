//! Immutable run-plan snapshot wire contract.

#[path = "engine/rig.rs"]
mod rig;

use bureau::adapters::{Execution, Usage};
use bureau::contract::StepOutcome;
use bureau::engine::RunPlan;
use bureau::runlog::{EventKind, RunLog, RunStartedData, run_started_snapshot};

#[test]
fn run_snapshot_round_trips_through_the_started_event() {
    let rig = rig::Rig::new();
    let _unused = (
        rig::step("unused", bureau::config::StepKind::Deterministic),
        rig::det_step("unused", "true", None),
        rig::agent_step("unused", "fixture", None),
        rig::decision_step("unused", "other"),
        rig::result(bureau::contract::StepOutcome::Success, "unused"),
        rig::fixture(
            rig.dir.path(),
            "unused.json",
            &rig::result(bureau::contract::StepOutcome::Success, "unused"),
        ),
        rig.engine(),
    );
    let plan = rig.plan(Vec::new());
    let value = run_started_snapshot(&plan.snapshot());
    let decoded: RunStartedData = serde_json::from_value(value).expect("snapshot decodes");
    assert_eq!(decoded.snapshot.expect("snapshot").run_id, plan.run_id);
}

#[test]
fn engine_discovers_and_rehydrates_unfinished_snapshot() {
    let rig = rig::Rig::new();
    let plan = rig.plan(Vec::new());
    let engine = rig.engine();
    let mut log = RunLog::create(&engine.runs_dir, &plan.run_id, &[]).expect("log");
    log.append(
        EventKind::RunStarted,
        run_started_snapshot(&plan.snapshot()),
    )
    .expect("started");
    log.close().expect("close");
    let snapshot = engine.unfinished().expect("unfinished").pop().expect("one");
    let restored = RunPlan::from_snapshot(snapshot, plan.forge.clone(), plan.credentials.clone());
    assert_eq!(
        (restored.run_id, restored.pipeline.name),
        (plan.run_id, plan.pipeline.name)
    );
}

#[tokio::test]
async fn resumed_step_receives_full_prior_outputs() {
    let rig = rig::Rig::new();
    let mut first = rig::agent_step("first", "not-used", Some("consume"));
    first.max_attempts = 2;
    let mut consume = rig::det_step(
        "consume",
        "grep -q hello && echo changed >> file.txt",
        Some("done"),
    );
    consume.inputs_from = vec!["first".to_owned()];
    let plan = rig.plan(vec![first, consume]);
    write_finished_first(&rig, &plan);
    let engine = rig.engine();
    let outcome = engine.run(&plan).await;
    let records = engine.finished().expect("finished");
    assert_eq!(
        (
            outcome.outcome,
            records.len(),
            records[0].snapshot.run_id.as_str()
        ),
        (StepOutcome::Success, 1, plan.run_id.as_str()),
        "{outcome:?}"
    );
}

fn write_finished_first(rig: &rig::Rig, plan: &RunPlan) {
    let engine = rig.engine();
    let mut log = RunLog::create(&engine.runs_dir, &plan.run_id, &[]).expect("log");
    log.append(
        EventKind::RunStarted,
        run_started_snapshot(&plan.snapshot()),
    )
    .expect("started");
    log.append(
        EventKind::StepStarted,
        bureau::runlog::step_started("first"),
    )
    .expect("step started");
    let mut result = rig::result(StepOutcome::Success, "first complete");
    result
        .outputs
        .insert("value".to_owned(), serde_json::json!("hello"));
    let execution = Execution::new(result, Usage::zero("fake"));
    log.append(
        EventKind::StepFinished,
        bureau::runlog::step_finished_full("first", &execution),
    )
    .expect("step finished");
    log.close().expect("close");
}
