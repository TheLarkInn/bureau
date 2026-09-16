#[path = "admission/continuation.rs"]
mod continuation;
#[path = "admission/credentials.rs"]
mod credentials;
#[path = "admission/gate.rs"]
mod gate;
#[path = "admission/initialization.rs"]
mod initialization;
#[path = "admission/support.rs"]
mod support;

use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

use bureau::forge::fake::FakeForge;
use bureau::runlog::{EventKind, RunStatus, preserved_factory_work, preserves_factory_work};
use bureau::state::{FreshClaim, LeaseOwner};

use super::fixture::Fixture;

fn fresh(fixture: &Fixture, assignment: &str, forge: &str, item: &str) -> FreshClaim {
    let owner = LeaseOwner::new(
        support::store(fixture),
        assignment,
        forge,
        item,
        "new-candidate",
    )
    .expect("fresh owner");
    let result = owner
        .claim_fresh(Duration::from_secs(30), &fixture.engine.runs_dir)
        .expect("guarded fresh admission");
    owner.release().expect("release only this candidate");
    result
}

async fn no_replacement(mode: &str) {
    let fixture = Fixture::create(mode);
    let _first = support::supervise(&fixture).await;
    let forge = Arc::new(FakeForge::new(vec![fixture.plan.item.clone()]));
    let reconciler = support::reconciler(&fixture, forge);
    let starts = support::repeated_passes(&reconciler).await;
    assert_eq!(
        (
            starts,
            support::directories(&fixture),
            fixture.calls("session.factory.run"),
            reconciler
                .state
                .active("offline")
                .expect("live leases")
                .len(),
        ),
        (vec![0, 0, 0], vec![fixture.plan.run_id.clone()], 1, 0),
    );
}

#[tokio::test]
async fn lost_acceptance_does_not_create_new_runs_after_supervisor_release() {
    no_replacement("ambiguous").await;
}

#[tokio::test]
async fn native_pause_does_not_create_new_runs_after_supervisor_release() {
    no_replacement("pause").await;
}

#[tokio::test]
async fn same_run_recovery_can_reclaim_without_freeing_a_paused_work_item() {
    let mut fixture = Fixture::create("pause");
    let _first = support::supervise(&fixture).await;
    fixture.plan.lease = Some(support::recovery_owner(&fixture));
    let _recovery = support::supervise(&fixture).await;
    assert_eq!(
        (
            fresh(&fixture, "offline", "github", "1"),
            support::directories(&fixture),
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume")
        ),
        (
            FreshClaim::PreservedFactory(fixture.plan.run_id.clone()),
            vec![fixture.plan.run_id.clone()],
            1,
            0
        ),
    );
}

#[tokio::test]
async fn admission_rechecks_when_work_changes_during_awaited_forge_observation() {
    let fixture = Fixture::create("ambiguous");
    let gate = gate::Gate::new(fixture.plan.item.clone());
    let reconciler = support::reconciler(&fixture, gate.clone());
    let mut task = tokio::spawn(async move {
        support::settle(reconciler.reconcile_once().await.expect("pass")).await
    });
    gate.wait(&mut task).await;
    let _first = support::supervise(&fixture).await;
    gate.release();
    assert_eq!(
        (
            task.await.expect("pass joins"),
            support::directories(&fixture),
            fixture.calls("session.factory.run")
        ),
        (0, vec![fixture.plan.run_id.clone()], 1),
    );
}

#[tokio::test]
async fn fresh_claim_reserves_only_the_matching_assignment_forge_and_item() {
    let fixture = Fixture::create("pause");
    let _first = support::supervise(&fixture).await;
    let cases = [
        (
            "offline",
            "github",
            "1",
            FreshClaim::PreservedFactory(fixture.plan.run_id.clone()),
        ),
        ("other", "github", "1", FreshClaim::Claimed),
        ("offline", "ado", "1", FreshClaim::Claimed),
        ("offline", "github", "2", FreshClaim::Claimed),
    ];
    for (assignment, forge, item, expected) in cases {
        assert_eq!(fresh(&fixture, assignment, forge, item), expected);
    }
}

#[tokio::test]
async fn expired_ownership_does_not_expire_unsettled_factory_work() {
    let fixture = Fixture::create("ambiguous");
    let _first = fixture.engine.run(&fixture.plan).await;
    let connection = rusqlite::Connection::open(fixture.root.join("state.db")).expect("database");
    connection
        .execute("UPDATE leases SET expires_at_ms = 0", [])
        .expect("expire owner");
    assert_eq!(
        fresh(&fixture, "offline", "github", "1"),
        FreshClaim::PreservedFactory(fixture.plan.run_id.clone()),
    );
}

#[tokio::test]
async fn a_finished_clean_factory_does_not_prevent_an_explicit_fresh_claim() {
    let fixture = Fixture::create("success");
    let _first = support::supervise(&fixture).await;
    assert_eq!(
        (
            fixture.count(EventKind::RunFinished),
            fresh(&fixture, "offline", "github", "1")
        ),
        (1, FreshClaim::Claimed),
    );
}

#[tokio::test]
async fn admission_ignores_stale_state_cache_and_never_repairs_a_torn_tail() {
    let fixture = Fixture::create("pause");
    let _first = support::supervise(&fixture).await;
    std::fs::write(
        fixture.directory().join("state.json"),
        r#"{"state":"finished"}"#,
    )
    .expect("poison only the derived cache");
    let events = fixture.directory().join("events.jsonl");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&events)
        .expect("append")
        .write_all(br#"{"seq":"#)
        .expect("torn tail");
    let before = std::fs::read(&events).expect("original bytes");
    let held = fresh(&fixture, "offline", "github", "1");
    assert_eq!(
        (held, std::fs::read(events).expect("unchanged bytes")),
        (
            FreshClaim::PreservedFactory(fixture.plan.run_id.clone()),
            before
        ),
    );
}

#[tokio::test]
async fn terminal_looking_outer_state_and_corrupt_logs_fail_closed() {
    let fixture = Fixture::create("pause");
    let _first = support::supervise(&fixture).await;
    let mut state = bureau::runlog::replay(fixture.events()).expect("state");
    state.status = RunStatus::Finished(bureau::contract::StepOutcome::Failure);
    assert!(
        preserves_factory_work(&state),
        "terminal-looking outer event cannot free unsafe work"
    );
    std::fs::write(fixture.directory().join("events.jsonl"), "invalid event\n")
        .expect("corrupt authoritative log");
    assert!(preserved_factory_work(&fixture.engine.runs_dir, "offline", "github").is_err());
}

#[tokio::test]
async fn missing_authoritative_snapshot_cannot_free_native_work() {
    let fixture = Fixture::create("pause");
    let _first = support::supervise(&fixture).await;
    let mut events = fixture.events();
    events[0].data["snapshot"] = serde_json::Value::Null;
    support::write_events(&fixture, &events);
    let error = preserved_factory_work(&fixture.engine.runs_dir, "offline", "github")
        .expect_err("cannot match preserved native work without the snapshot");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn a_missing_log_inside_an_existing_run_directory_fails_closed() {
    let fixture = Fixture::create("pause");
    let _first = support::supervise(&fixture).await;
    std::fs::remove_file(fixture.directory().join("events.jsonl")).expect("remove fixture log");
    let error = preserved_factory_work(&fixture.engine.runs_dir, "offline", "github")
        .expect_err("an existing run is not evidence of no preserved work");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[tokio::test]
async fn ordinary_unfinished_runs_retain_their_existing_admission_policy() {
    let fixture = Fixture::create("pause");
    let _first = support::supervise(&fixture).await;
    let mut state = bureau::runlog::replay(fixture.events()).expect("state");
    state.snapshot.as_mut().expect("snapshot").pipeline.steps[0].copilot_factory = None;
    state.copilot_factories.0.clear();
    assert!(!preserves_factory_work(&state));
}
