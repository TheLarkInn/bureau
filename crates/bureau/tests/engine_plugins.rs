//! Engine-level plugin pinning and integrity checks, entirely offline.

#[path = "engine/rig.rs"]
mod rig;

use std::sync::Arc;
use std::time::Duration;

use bureau::adapters::AdapterKind;
use bureau::contract::StepOutcome;
use bureau::forge::Forge as _;
use bureau::runlog::{self, EventKind};
use bureau::state::{LeaseOwner, Store};
use rig::{Rig, agent_step, det_step, fixture, result};

#[tokio::test]
async fn missing_plugin_fails_before_any_pipeline_step() {
    let rig = Rig::new();
    let _unused = rig::decision_step("unused", "unused");
    let mut plan = rig.plan(vec![
        det_step("write", "touch should-not-run", Some("done")),
        agent_step("unused", "unused", Some("done")),
    ]);
    let role = plan.roles.get_mut("worker").expect("worker role");
    role.adapter = AdapterKind::Copilot;
    role.agent = "/missing-plugin:worker".to_owned();
    let outcome = rig.engine().run(&plan).await;
    let events = events(&rig, &outcome.run_id);
    let started = events
        .iter()
        .filter(|event| event.kind == EventKind::StepStarted)
        .count();
    assert_eq!((outcome.outcome, started), (StepOutcome::Failure, 0));
}

#[tokio::test]
async fn plugin_identity_is_pinned_before_steps_execute() {
    let rig = Rig::new();
    let mut stop = det_step("stop", "false", Some("unused"));
    stop.on_failure = Some("abort".to_owned());
    let mut plan = rig.plan(vec![stop, agent_step("unused", "unused", Some("done"))]);
    let role = plan.roles.get_mut("worker").expect("worker role");
    role.adapter = AdapterKind::Copilot;
    role.agent = "/bureau:implementer".to_owned();
    let outcome = rig.engine().run(&plan).await;
    let state =
        runlog::replay_state(&rig.dir.path().join("runs").join(&outcome.run_id)).expect("replay");
    let source = state
        .snapshot
        .expect("snapshot")
        .plugin_sources
        .remove("bureau")
        .expect("bureau source");
    assert_eq!(
        (outcome.outcome, source.name, source.digest.is_empty()),
        (StepOutcome::Failure, "bureau".to_owned(), false)
    );
}

#[tokio::test]
async fn agent_step_rejects_a_changed_pinned_plugin() {
    let rig = Rig::new();
    let transcript = fixture(
        rig.dir.path(),
        "unused-plugin-agent.json",
        &result(StepOutcome::Success, "should not run"),
    );
    let corrupt = "printf changed > ../plugins/bureau/tree/agents/implementer.agent.md";
    let mut agent = agent_step("agent", &transcript, Some("done"));
    agent.on_blocked = Some("done".to_owned());
    let mut plan = rig.plan(vec![det_step("corrupt", corrupt, Some("agent")), agent]);
    plan.roles.get_mut("worker").expect("worker role").agent = "/bureau:implementer".to_owned();
    let outcome = rig.engine().run(&plan).await;
    let prs = rig.forge.open_prs("main", "bureau/").await.expect("prs");
    assert_eq!(
        (outcome.outcome, prs.len(), rig.forge.comments().len()),
        (StepOutcome::Blocked, 0, 1)
    );
}

#[tokio::test]
async fn absolute_agent_path_is_not_treated_as_a_plugin() {
    let rig = Rig::new();
    let mut plan = rig.plan(vec![
        det_step("check", "true", Some("done")),
        agent_step("unused", "unused", Some("done")),
    ]);
    plan.roles.get_mut("worker").expect("worker role").agent = "/tmp/worker.md".to_owned();
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::NoWork);
}

#[tokio::test]
async fn expired_supervisor_cannot_publish_after_takeover() {
    let rig = Rig::new();
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let mut plan = rig.plan(vec![det_step(
        "edit",
        "echo changed >> file.txt; sleep 0.3",
        Some("done"),
    )]);
    let first = owner(store.clone(), &plan.run_id);
    first.claim(Duration::from_millis(50)).expect("claim");
    plan.lease = Some(first);
    let engine = rig.engine();
    let run = engine.run(&plan);
    let takeover = take_over(store, plan.run_id.clone());
    let (outcome, second) = tokio::join!(run, takeover);
    let prs = rig.forge.open_prs("main", "bureau/").await.expect("prs");
    assert_eq!(
        (outcome.outcome, second.owns().expect("owns"), prs.len()),
        (StepOutcome::Failure, true, 0)
    );
}

#[tokio::test]
async fn dropping_engine_future_aborts_the_inflight_process() {
    let rig = Rig::new();
    let marker = rig.dir.path().join("late-side-effect");
    let command = format!("sleep 0.4; printf late > '{}'", marker.display());
    let plan = rig.plan(vec![det_step("late", &command, Some("done"))]);
    let engine = rig.engine();
    let timed = tokio::time::timeout(Duration::from_millis(100), engine.run(&plan)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(timed.is_err() && !marker.exists());
}

#[tokio::test]
async fn missing_terminal_event_fails_supervision_and_releases() {
    let rig = Rig::new();
    let store = Arc::new(Store::open_in_memory().expect("store"));
    let mut plan = rig.plan(vec![det_step("wait", "sleep 0.3", Some("done"))]);
    let owner = owner(store.clone(), &plan.run_id);
    owner.claim(Duration::from_secs(60)).expect("claim");
    plan.lease = Some(owner);
    let events = rig
        .dir
        .path()
        .join("runs")
        .join(&plan.run_id)
        .join(runlog::EVENTS_FILE);
    let engine = Arc::new(rig.engine());
    let supervised = bureau::supervise::run(engine, store.clone(), plan);
    let removed = remove_when_created(events);
    let ((outcome, projection), ()) = tokio::join!(supervised, removed);
    assert!(
        outcome.outcome == StepOutcome::Failure
            && projection.is_err()
            && store.active("fix-tests").expect("active").is_empty()
    );
}

async fn remove_when_created(path: std::path::PathBuf) {
    while !path.exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    std::fs::remove_file(path).expect("remove events");
}

async fn take_over(store: Arc<Store>, run_id: String) -> LeaseOwner {
    tokio::time::sleep(Duration::from_millis(120)).await;
    let owner = owner(store, &run_id);
    assert!(owner.claim(Duration::from_secs(60)).expect("takeover"));
    owner
}

fn owner(store: Arc<Store>, run_id: &str) -> LeaseOwner {
    LeaseOwner::new(store, "fix-tests", "github", "42", run_id).expect("owner")
}

fn events(rig: &Rig, run_id: &str) -> Vec<runlog::Event> {
    let directory = rig.dir.path().join("runs").join(run_id);
    runlog::read_events(&directory).expect("events")
}
