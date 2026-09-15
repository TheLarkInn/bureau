use std::time::Duration;

use bureau::adapters::copilot_factory::types::FactoryRunStatus;
use bureau::contract::StepOutcome;
use bureau::runlog::EventKind;

use super::fixture::Fixture;

fn check_unconfirmed_cancel(fixture: &Fixture, observations: usize) {
    let record = fixture.record();
    assert_eq!(
        (record.status(), record.can_clean(), record.can_resume()),
        (Some(FactoryRunStatus::Paused), false, true)
    );
    assert_eq!(
        (
            fixture.calls("session.factory.cancel"),
            fixture.calls("session.factory.resume"),
            fixture.calls("session.factory.getRun") - observations,
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (1, 0, 2, 0, 0)
    );
}

#[tokio::test]
async fn unowned_cancellation_preserves_the_stopped_run_without_claiming_it_was_cancelled() {
    let mut fixture = Fixture::create("unowned-cancel");
    fixture.plan.pipeline.steps[0].timeout_secs = Some(4);
    let _first = fixture.engine.run(&fixture.plan).await;
    let observations = fixture.calls("session.factory.getRun");
    std::fs::write(fixture.directory().join("CANCEL"), "explicit cancel").expect("cancel marker");
    let _second = fixture.engine.run(&fixture.plan).await;
    check_unconfirmed_cancel(&fixture, observations);
}

#[tokio::test]
async fn cancellation_between_result_and_detail_is_read_again_before_settlement() {
    let fixture = Fixture::create("pause-cancel-race");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.pause"),
            fixture.calls("session.factory.cancel"),
            fixture.record().status(),
            fixture.count(EventKind::RunFinished)
        ),
        (1, 1, Some(FactoryRunStatus::Cancelled), 1),
        "{outcome:?}"
    );
    assert!(
        fixture
            .trace()
            .iter()
            .any(|entry| entry["race"] == "paused-result-before-cancel")
    );
}

#[tokio::test]
async fn cancel_can_supersede_an_in_flight_native_pause() {
    let fixture = Fixture::create("pause-then-cancel");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.pause"),
            fixture.calls("session.factory.cancel"),
            fixture.record().status(),
            fixture.count(EventKind::RunFinished)
        ),
        (1, 1, Some(FactoryRunStatus::Cancelled), 1),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn cancellation_waits_for_native_settlement_and_clean_shutdown() {
    let fixture = Fixture::create("cancel");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            outcome.outcome,
            fixture.record().status(),
            fixture.calls("session.factory.cancel"),
            fixture.count(EventKind::StepFinished),
            fixture.directory().join("wt").exists()
        ),
        (
            StepOutcome::Failure,
            Some(FactoryRunStatus::Cancelled),
            1,
            1,
            false
        )
    );
}

#[tokio::test]
async fn confirmed_cancellation_on_reentry_never_starts_a_native_resume_attempt() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    std::fs::write(fixture.directory().join("CANCEL"), "explicit cancel").expect("cancel marker");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().status(),
            fixture.calls("session.factory.cancel"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (Some(FactoryRunStatus::Cancelled), 1, 0, 1, 1)
    );
}

#[tokio::test]
async fn unadmitted_bootstrap_reentry_preserves_the_pause_and_native_identity() {
    let fixture = Fixture::create("bootstrap-pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let prior = (fixture.record(), fixture.trace());
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            (fixture.record(), fixture.trace()),
            fixture.count(EventKind::StepStarted),
            fixture.count(EventKind::StepFinished),
            fixture.directory().join("PAUSE").is_file()
        ),
        (prior, 1, 0, true)
    );
}

#[tokio::test]
async fn pause_is_dispatched_while_an_authoritative_read_is_still_pending() {
    let fixture = Fixture::create("delayed-observation-pause");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().status(),
            fixture.calls("session.factory.pause"),
            fixture.count(EventKind::StepFinished)
        ),
        (Some(FactoryRunStatus::Paused), 1, 0)
    );
}

#[tokio::test]
async fn hosted_permissions_are_denied_without_blocking_the_pending_factory_call() {
    let fixture = Fixture::create("permissions");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            outcome.outcome,
            fixture.record().can_clean(),
            fixture.calls("session.permissions.handlePendingPermissionRequest")
        ),
        (StepOutcome::NoWork, true, 2),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn native_halt_is_unfinished_and_can_resume_without_new_limits() {
    let fixture = Fixture::create("halted");
    let _first = fixture.engine.run(&fixture.plan).await;
    let halted = fixture.record();
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            halted.status(),
            halted.can_resume(),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepStarted),
            second.cost_usd
        ),
        (Some(FactoryRunStatus::Halted), true, 1, 1, 0.02)
    );
}

#[tokio::test]
async fn hung_native_pause_remains_bounded_by_the_process_deadline() {
    let mut fixture = Fixture::create("pause-hang");
    fixture.plan.pipeline.steps[0].timeout_secs = Some(4);
    let started = tokio::time::Instant::now();
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.pause"),
            fixture.record().can_resume(),
            fixture.count(EventKind::StepFinished),
            fixture.directory().join("wt").exists()
        ),
        (1, false, 0, true),
        "{outcome:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(10), "{outcome:?}");
}

#[tokio::test]
async fn deadline_kills_factory_descendants_and_preserves_unsettled_state() {
    let mut fixture = Fixture::create("hard-timeout");
    fixture.plan.pipeline.steps[0].timeout_secs = Some(4);
    let listener = super::notify::listener(&fixture, "liveness-socket");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    super::notify::closed(listener).await;
    let record = fixture.record();
    assert_eq!(
        (
            record.execution_clean,
            record
                .intent
                .paths
                .storage
                .session
                .join("orphan-survived")
                .exists(),
            fixture.count(EventKind::RunFinished)
        ),
        (false, false, 0)
    );
}
