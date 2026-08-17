//! Run deadline, live approval, and idempotent terminal recovery.

#[path = "engine/rig.rs"]
mod rig;

use std::time::Duration;

use bureau::contract::{StepOutcome, Trust};
use bureau::forge::Forge;
use bureau::runlog::{self, EventKind};

#[tokio::test]
async fn zero_hour_deadline_escalates_before_a_step_spawns() {
    let rig = rig::Rig::new();
    use_fixture_helpers(&rig);
    let mut plan = rig.plan(vec![rig::det_step("never", "exit 99", Some("done"))]);
    plan.assignment.limits.max_run_hours = Some(0);
    let outcome = rig.engine().run(&plan).await;
    assert!(
        outcome.outcome == StepOutcome::Blocked && outcome.message.contains("0 hour deadline"),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn removing_approval_blocks_the_next_step() {
    let rig = rig::Rig::new();
    let labels = vec!["agent-approved".to_owned()];
    rig.forge.set_labels("42", &labels).await.expect("approve");
    let steps = vec![
        rig::det_step(
            "first",
            "sleep 0.4; echo changed >> file.txt",
            Some("second"),
        ),
        rig::det_step("second", "echo should-not-run >> file.txt", Some("done")),
    ];
    let mut plan = rig.plan(steps);
    plan.assignment.work.approval_label = Some("agent-approved".to_owned());
    let engine = rig.engine();
    let task = tokio::spawn(async move { engine.run(&plan).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    rig.forge.set_labels("42", &[]).await.expect("revoke");
    let outcome = task.await.expect("run task");
    assert_eq!(outcome.outcome, StepOutcome::Blocked, "{outcome:?}");
}

#[tokio::test]
async fn crash_after_pr_creation_adopts_the_existing_pr() {
    let rig = rig::Rig::new();
    let plan = rig.plan(vec![rig::det_step(
        "edit",
        "echo changed >> file.txt",
        Some("done"),
    )]);
    let first = rig.engine().run(&plan).await;
    remove_finished_event(rig.dir.path(), &plan.run_id);
    let second = rig.engine().run(&plan).await;
    let prs = rig
        .forge
        .open_prs(&rig.url, "bureau/")
        .await
        .expect("open prs");
    assert_eq!(
        (first.outcome, second.outcome, prs.len()),
        (StepOutcome::Success, StepOutcome::Success, 1)
    );
}

fn remove_finished_event(root: &std::path::Path, run_id: &str) {
    let dir = root.join("runs").join(run_id);
    let events = runlog::read_events(&dir).expect("events");
    let kept: Vec<_> = events
        .into_iter()
        .filter(|event| event.kind != EventKind::RunFinished)
        .collect();
    let mut bytes = Vec::new();
    for event in kept {
        bytes.extend(serde_json::to_vec(&event).expect("event"));
        bytes.push(b'\n');
    }
    std::fs::write(dir.join(runlog::EVENTS_FILE), bytes).expect("rewrite log");
}

fn use_fixture_helpers(rig: &rig::Rig) {
    let result = rig::result(StepOutcome::Success, "unused");
    let fixture = rig::fixture(rig.dir.path(), "unused.json", &result);
    let _ = (
        rig::step("unused", bureau::config::StepKind::Deterministic),
        rig::agent_step("unused", &fixture, None),
        rig::decision_step("unused", "other"),
        Trust::Derived,
    );
}
