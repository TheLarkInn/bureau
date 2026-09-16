use bureau::runlog::EventKind;
use bureau::runlog::copilot_factory::{Record, Workspace};

use super::fixture::Fixture;

fn count(fixture: &Fixture, kind: EventKind) -> usize {
    fixture
        .events()
        .iter()
        .filter(|event| event.kind == kind)
        .count()
}

fn calls(fixture: &Fixture, method: &str) -> usize {
    fixture
        .trace()
        .iter()
        .filter(|entry| entry["method"] == method)
        .count()
}

fn identity(record: &Record) -> (Option<&str>, &str, &Workspace) {
    (
        record.run_id.as_deref(),
        &record.intent.session_id,
        &record.intent.workspace,
    )
}

#[tokio::test]
async fn a_complete_last_event_without_a_newline_keeps_native_recovery_separate() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let events = fixture.directory().join(bureau::runlog::EVENTS_FILE);
    let bytes = std::fs::read(&events).expect("event bytes");
    std::fs::write(&events, bytes.strip_suffix(b"\n").expect("last newline"))
        .expect("complete event without its delimiter");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepStarted),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (1, 1, 1, 1, 1)
    );
}

#[tokio::test]
async fn ambiguous_start_preserves_workspace_and_never_retries() {
    let fixture = Fixture::create("ambiguous");
    let _first = fixture.engine.run(&fixture.plan).await;
    let record = fixture.record();
    assert_eq!(
        (
            record.ambiguous_start(),
            record.run_id,
            fixture.directory().join("wt").exists()
        ),
        (true, None, true)
    );
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            calls(&fixture, "session.factory.run"),
            count(&fixture, EventKind::StepStarted),
            count(&fixture, EventKind::StepFinished),
            count(&fixture, EventKind::RunFinished)
        ),
        (1, 1, 0, 0)
    );
}

#[tokio::test]
async fn pause_resumes_same_native_identity_and_bureau_attempt() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let original = fixture.record();
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let second = fixture.engine.run(&fixture.plan).await;
    let resumed = fixture.record();
    assert_eq!(identity(&original), identity(&resumed));
    assert_eq!(
        (
            resumed.attempt.map(std::num::NonZeroU64::get),
            second.cost_usd,
            count(&fixture, EventKind::StepStarted),
            count(&fixture, EventKind::StepFinished)
        ),
        (Some(2), 0.02, 1, 1),
        "{second:?}"
    );
}

#[tokio::test]
async fn completed_result_without_shutdown_ack_is_not_adopted() {
    let fixture = Fixture::create("unacknowledged-shutdown");
    let _first = fixture.engine.run(&fixture.plan).await;
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit inspection");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            calls(&fixture, "session.factory.run"),
            calls(&fixture, "session.factory.resume"),
            count(&fixture, EventKind::StepFinished),
            fixture.record().can_clean()
        ),
        (1, 0, 0, false)
    );
}
