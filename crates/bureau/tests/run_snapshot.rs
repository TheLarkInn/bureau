//! Immutable run-plan snapshot wire contract.

#[path = "engine/rig.rs"]
mod rig;

use std::sync::Arc;
use std::time::Duration;

use bureau::adapters::{Execution, Usage};
use bureau::contract::StepOutcome;
use bureau::engine::{RunPlan, TerminalRecord};
use bureau::forge::Pr;
use bureau::runlog::RunFinishedData;
use bureau::runlog::TerminalDisposition;
use bureau::runlog::{EventKind, RunLog, RunStartedData, run_started_snapshot};
use bureau::state::{LeaseOwner, Store};

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
    let mut plan = rig.plan(Vec::new());
    plan.direct_agents
        .insert("worker".to_owned(), b"agent".to_vec());
    let value = run_started_snapshot(&plan.snapshot());
    let decoded: RunStartedData = serde_json::from_value(value).expect("snapshot decodes");
    let snapshot = decoded.snapshot.expect("snapshot");
    assert_eq!(
        (snapshot.run_id, snapshot.direct_agents),
        (plan.run_id, plan.direct_agents)
    );
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

#[test]
fn blocked_recovery_preserves_cost_and_pull_request() {
    let rig = rig::Rig::new();
    let plan = rig.plan(Vec::new());
    let engine = rig.engine();
    let pr = recovery_pr();
    write_recovery_history(&engine, &plan, &pr);
    engine
        .block(&plan.snapshot(), "credential missing")
        .expect("block");
    let finished = engine.finished().expect("finished").pop().expect("one");
    assert_eq!(
        (
            finished.finished.cost_usd,
            finished.finished.pr,
            finished.finished.disposition,
        ),
        (4.25, Some(pr), Some(TerminalDisposition::Proposed))
    );
}

#[test]
fn terminal_projection_releases_any_generation_for_the_run() {
    let rig = rig::Rig::new();
    let plan = rig.plan(Vec::new());
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let owner = LeaseOwner::new(
        store.clone(),
        &plan.assignment.name,
        "github",
        &plan.item.external_id,
        &plan.run_id,
    )
    .expect("owner");
    owner.claim(Duration::from_secs(60)).expect("claim");
    let record = terminal_record(&plan);
    bureau::state::project_terminal(&store, &record).expect("project");
    assert!(
        store
            .active(&plan.assignment.name)
            .expect("active")
            .is_empty()
    );
}

fn terminal_record(plan: &RunPlan) -> TerminalRecord {
    TerminalRecord {
        snapshot: plan.snapshot(),
        finished: RunFinishedData {
            outcome: StepOutcome::NoWork,
            message: "done".to_owned(),
            cost_usd: 0.0,
            pr: None,
            disposition: Some(TerminalDisposition::NoChange),
        },
    }
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

fn write_recovery_history(engine: &bureau::engine::Engine, plan: &RunPlan, pr: &Pr) {
    let mut log = RunLog::create(&engine.runs_dir, &plan.run_id, &[]).expect("log");
    log.append(
        EventKind::RunStarted,
        run_started_snapshot(&plan.snapshot()),
    )
    .expect("started");
    log.append(EventKind::StepStarted, bureau::runlog::step_started("work"))
        .expect("step");
    let mut usage = Usage::zero("fake");
    usage.cost_usd = Some(4.25);
    let execution = Execution::new(rig::result(StepOutcome::Success, "done"), usage);
    log.append(
        EventKind::StepFinished,
        bureau::runlog::step_finished_full("work", &execution),
    )
    .expect("finished");
    log.append(EventKind::PrCreated, bureau::runlog::pr_created(pr, "abc"))
        .expect("pr");
    log.close().expect("close");
}

fn recovery_pr() -> Pr {
    Pr {
        number: 7,
        repo: "main".to_owned(),
        branch: "bureau/run".to_owned(),
        title: "Fix".to_owned(),
        url: "fake://pr/7".to_owned(),
        item_id: Some("42".to_owned()),
    }
}
