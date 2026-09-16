use std::sync::Arc;

use bureau::adapters::copilot_factory::types::FactoryRunStatus;
use bureau::forge::fake::FakeForge;
use bureau::reconcile::{Error, Reconciler};
use bureau::runlog::{RunState, RunStatus};

use super::super::fixture::Fixture;
use super::{gate, support};

fn unclaimed() -> Fixture {
    let fixture = Fixture::create("success");
    fixture
        .plan
        .lease
        .as_ref()
        .expect("initial fixture lease")
        .release()
        .expect("let Reconciler make its own fresh claim");
    fixture
}

fn model_reference(fixture: &Fixture) -> &str {
    fixture
        .plan
        .pipeline
        .factory_credential_refs()
        .next()
        .expect("explicit factory model reference")
}

fn without_model(fixture: &Fixture, forge: &Arc<gate::Gate>) -> Arc<Reconciler> {
    let mut reconciler = support::reconciler(fixture, forge.clone());
    Arc::get_mut(&mut reconciler)
        .expect("unshared reconciler")
        .credentials
        .remove(model_reference(fixture));
    reconciler
}

async fn completed(fixture: &mut Fixture, reconciler: &Reconciler) -> RunState {
    let mut started = reconciler.reconcile_once().await.expect("admitted pass");
    assert_eq!(started.len(), 1, "one item produces one Bureau run");
    let run = started.pop().expect("one admitted run");
    fixture.plan.run_id = run.run_id;
    run.handle.await.expect("supervised factory joins");
    bureau::runlog::replay_state(&fixture.directory()).expect("replay actual Engine events")
}

#[tokio::test]
async fn missing_model_reference_blocks_before_forge_observation_or_fresh_identity() {
    let fixture = unclaimed();
    let forge = gate::Gate::new(fixture.plan.item.clone());
    let reconciler = without_model(&fixture, &forge);
    let error = forge
        .before_query(reconciler.reconcile_once())
        .await
        .err()
        .expect("unresolved reference prevents dispatch");
    assert_eq!(
        (
            matches!(error, Error::ModelCredential(reference) if reference == model_reference(&fixture)),
            reconciler.state.active("offline").expect("leases").len(),
            reconciler.engine.runs_dir.exists(),
        ),
        (true, 0, false),
    );
}

#[tokio::test]
async fn resolved_model_reference_reaches_the_actual_reconciled_factory_engine() {
    let mut fixture = unclaimed();
    let forge = Arc::new(FakeForge::new(vec![fixture.plan.item.clone()]));
    let reconciler = support::reconciler(&fixture, forge);
    let state = completed(&mut fixture, &reconciler).await;
    let record = state.copilot_factories.0.values().next().expect("factory");
    assert_eq!(
        (
            record.status(),
            record.intent.factory.model_credential.as_str(),
            matches!(state.status, RunStatus::Finished(_)),
            state.copilot_factory_error,
            fixture.calls("session.factory.run"),
        ),
        (
            Some(FactoryRunStatus::Completed),
            model_reference(&fixture),
            true,
            None,
            1,
        ),
    );
}
