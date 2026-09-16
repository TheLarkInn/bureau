use bureau::contract::{StepOutcome, Trust};
use bureau::runlog::EventKind;

use super::fixture::Fixture;

#[tokio::test]
async fn durable_factory_logs_preserve_an_existing_empty_pipeline_layout() {
    let fixture = Fixture::create("success");
    std::fs::create_dir_all(fixture.directory().join("artifacts")).expect("artifact directory");
    std::fs::create_dir_all(fixture.directory().join("wt")).expect("empty worktree directory");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (outcome.outcome, fixture.calls("session.factory.run")),
        (StepOutcome::NoWork, 1),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_qualified_distribution_at_the_bundle_root_runs_without_dot_aliases() {
    let fixture = Fixture::create("root-dist");
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (outcome.outcome, fixture.calls("session.factory.run")),
        (StepOutcome::NoWork, 1),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn engine_executes_one_native_factory_to_clean_completion() {
    let fixture = Fixture::create("success");
    let outcome = fixture.engine.run(&fixture.plan).await;
    let record = fixture.record();
    let state = bureau::runlog::replay_state(&fixture.directory()).expect("run state");
    let step = &state.steps[0];
    assert_eq!(
        (outcome.outcome, record.can_clean(), outcome.cost_usd),
        (StepOutcome::NoWork, true, 0.01),
        "{outcome:?}"
    );
    assert_eq!(
        (
            step.outcome,
            step.result.as_ref().map(|result| result.trust)
        ),
        (Some(StepOutcome::Success), Some(Trust::Derived))
    );
    assert!(!fixture.directory().join("wt").exists());
}

#[tokio::test]
async fn completed_factory_reentry_never_starts_another_native_run() {
    let fixture = Fixture::create("success");
    let first = fixture.engine.run(&fixture.plan).await;
    let before = fixture.events();
    let second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (first.outcome, first.cost_usd),
        (second.outcome, second.cost_usd)
    );
    assert!(
        before == fixture.events(),
        "finished reentry changed the event log"
    );
}

#[tokio::test]
async fn only_the_terminal_result_document_can_finish_the_step() {
    let fixture = Fixture::create("null-result");
    let outcome = fixture.engine.run(&fixture.plan).await;
    let finished = fixture
        .events()
        .iter()
        .filter(|event| event.kind == EventKind::StepFinished)
        .count();
    assert_eq!(
        (outcome.outcome, finished),
        (StepOutcome::Failure, 1),
        "{outcome:?}"
    );
}
