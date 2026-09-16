use bureau::engine::Engine;
use bureau::runlog::EventKind;

use super::{fixture::Fixture, notify};

#[tokio::test]
async fn a_live_lease_for_another_run_cannot_authorize_this_factory() {
    let mut fixture = Fixture::create("success");
    fixture.plan.run_id.push_str("-different");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (outcome.outcome, fixture.directory().exists()),
        (bureau::contract::StepOutcome::Failure, false)
    );
}

#[tokio::test]
async fn a_factory_cannot_create_its_log_without_an_actual_scheduler_lease() {
    let mut fixture = Fixture::create("success");
    fixture.plan.lease = None;
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (outcome.outcome, fixture.directory().exists()),
        (bureau::contract::StepOutcome::Failure, false)
    );
}

fn running(fixture: &Fixture) -> tokio::task::JoinHandle<bureau::engine::RunOutcome> {
    let engine = Engine::new(fixture.root.join("runs"), fixture.root.join("cache"));
    let plan = fixture.plan.clone();
    tokio::spawn(async move { engine.run(&plan).await })
}

async fn interrupt(fixture: &Fixture) {
    let listener = notify::listener(fixture, "admission-socket");
    let mut task = running(fixture);
    assert!(
        notify::admitted(&listener, &mut task).await,
        "engine exited before native admission"
    );
    task.abort();
    let _cancelled = task.await;
}

#[tokio::test]
async fn dropped_engine_execution_can_only_inspect_the_known_native_id() {
    let fixture = Fixture::create("hard-timeout");
    interrupt(&fixture).await;
    let _reentered = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepStarted),
            fixture.count(EventKind::StepFinished),
            fixture.record().execution_clean
        ),
        (1, 0, 1, 0, false)
    );
}

#[tokio::test]
async fn losing_the_scheduler_generation_never_admits_a_native_resume() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    fixture
        .plan
        .lease
        .as_ref()
        .expect("owner")
        .release()
        .expect("lose lease");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let before = fixture.events();
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(fixture.calls("session.factory.resume"), 0);
    assert!(
        before == fixture.events(),
        "stale owner appended factory facts"
    );
}
