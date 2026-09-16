//! Actual engine routing after a correlated SDK admission rejection.

#[path = "corrections/pause_projection.rs"]
mod pause_projection;

use bureau::runlog::EventKind;
use serde_json::{Value, json};

use super::fixture::Fixture;

fn failure_route(fixture: &mut Fixture) {
    fixture.plan.pipeline.steps[0].on_failure = Some("failure-route".into());
    fixture.plan.pipeline.steps.push(
        serde_json::from_value(json!({
            "name": "failure-route", "type": "deterministic",
            "run": "true", "next": "done"
        }))
        .expect("configured failure route"),
    );
}

fn steps(fixture: &Fixture) -> Vec<Value> {
    fixture
        .events()
        .into_iter()
        .filter(|event| event.kind == EventKind::StepFinished)
        .map(|event| json!([event.data["step"], event.data["outcome"]]))
        .collect()
}

fn native_events(fixture: &Fixture, name: &str) -> usize {
    fixture
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::CopilotFactory && event.data["event"] == name)
        .count()
}

fn sdk_state(fixture: &Fixture) -> Value {
    let path = fixture
        .record()
        .intent
        .paths
        .storage
        .session
        .join("session.db");
    let connection = rusqlite::Connection::open(path).expect("offline SDK storage");
    let data: String = connection
        .query_row("SELECT data FROM fixture WHERE id = 1", [], |row| {
            row.get(0)
        })
        .expect("offline SDK state");
    serde_json::from_str(&data).expect("offline SDK state JSON")
}

fn check_rejection(fixture: &Fixture) {
    let record = fixture.record();
    assert_eq!(
        (
            (record.rejected.as_deref(), record.run_id.as_deref()),
            (
                fixture.calls("session.factory.run"),
                fixture.calls("session.factory.getRun"),
                fixture.calls("runtime.shutdown")
            ),
            (
                native_events(fixture, "accepted"),
                native_events(fixture, "notification")
            ),
            sdk_state(fixture)
        ),
        (
            (
                Some("factory_not_found: synthetic definite admission rejection"),
                None
            ),
            (1, 0, 1),
            (0, 0),
            json!({"attempt": 0, "status": "pending", "mode": null})
        )
    );
}

fn terminal(fixture: &Fixture) -> Option<Value> {
    fixture
        .events()
        .into_iter()
        .find(|event| event.kind == EventKind::RunFinished)
        .map(|event| event.data["terminal"].clone())
}

#[tokio::test]
async fn definite_rejection_takes_the_failure_route_after_clean_shutdown() {
    let mut fixture = Fixture::create("rejection-clean");
    failure_route(&mut fixture);
    let _outcome = fixture.engine.run(&fixture.plan).await;
    check_rejection(&fixture);
    assert_eq!(
        (
            fixture.record().execution_clean,
            fixture.record().can_clean(),
            fixture.directory().join("PAUSE").exists(),
            steps(&fixture),
            terminal(&fixture)
        ),
        (
            true,
            true,
            false,
            vec![
                json!(["factory-step", "failure"]),
                json!(["failure-route", "success"])
            ],
            Some(json!("done"))
        )
    );
}

fn check_operator_pause(fixture: &Fixture) {
    check_rejection(fixture);
    assert_eq!(
        (
            fixture.record().execution_clean,
            fixture.record().can_clean(),
            std::fs::read_to_string(fixture.directory().join("PAUSE")).expect("operator marker"),
            steps(fixture),
            fixture.count(EventKind::RunFinished)
        ),
        (
            true,
            true,
            "operator pause\n".into(),
            vec![json!(["factory-step", "failure"])],
            0
        )
    );
}

#[tokio::test]
async fn definite_rejection_preserves_operator_pause_before_and_during_shutdown() {
    for mode in ["rejection-operator-pause", "rejection-shutdown-pause"] {
        let mut fixture = Fixture::create(mode);
        failure_route(&mut fixture);
        let _outcome = fixture.engine.run(&fixture.plan).await;
        check_operator_pause(&fixture);
    }
}

fn check_unclean(fixture: &Fixture) {
    check_rejection(fixture);
    let record = fixture.record();
    assert_eq!(
        (
            record.execution_clean,
            record.can_clean(),
            record.indeterminate.is_some(),
            fixture.directory().join("PAUSE").is_file(),
            fixture.directory().join("wt").is_dir(),
            steps(fixture),
            fixture.count(EventKind::RunFinished)
        ),
        (false, false, true, true, true, vec![], 0)
    );
}

#[tokio::test]
async fn rejection_without_acknowledged_clean_shutdown_preserves_the_run() {
    for mode in [
        "rejection-unacknowledged-shutdown",
        "rejection-unclean-shutdown",
    ] {
        let mut fixture = Fixture::create(mode);
        failure_route(&mut fixture);
        let _outcome = fixture.engine.run(&fixture.plan).await;
        check_unclean(&fixture);
    }
}

#[tokio::test]
async fn uncertain_acceptance_never_follows_the_failure_route() {
    let mut fixture = Fixture::create("ambiguous");
    failure_route(&mut fixture);
    let _outcome = fixture.engine.run(&fixture.plan).await;
    let record = fixture.record();
    assert_eq!(
        (
            record.rejected.as_deref(),
            record.ambiguous_start(),
            record.can_clean(),
            fixture.calls("session.factory.run"),
            steps(&fixture),
            fixture.directory().join("PAUSE").is_file(),
            fixture.count(EventKind::RunFinished)
        ),
        (None, true, false, 1, vec![], true, 0)
    );
}
