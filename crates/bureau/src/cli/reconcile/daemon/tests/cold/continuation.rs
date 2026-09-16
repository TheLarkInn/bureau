use std::path::PathBuf;

use bureau::engine::RunPlan;
use bureau::process::Secret;
use bureau::setup::CredentialSource;

use super::super::native_source;
use super::evidence::Evidence;
use super::fixture::{Paused, prepare};
use super::{World, idle_failure, independent_work};

const FUTURE_REFERENCE: &str = "z-future-model";

fn future_step(plan: &mut RunPlan) {
    let mut step = plan.pipeline.steps[0].clone();
    step.name = "future-factory-step".into();
    step.copilot_factory
        .as_mut()
        .expect("factory step")
        .model_credential = FUTURE_REFERENCE.into();
    plan.pipeline.steps[0].next = Some(step.name.clone());
    plan.pipeline.steps.push(step);
}

fn future_source(world: &mut World, plan: &mut RunPlan) -> PathBuf {
    let source = world.root().join("future-model-credential");
    native_source::write(&source, "offline-future-model-token");
    world
        .daemon
        .settings
        .as_mut()
        .expect("fixture settings")
        .credentials
        .insert(
            FUTURE_REFERENCE.into(),
            CredentialSource::File {
                path: source.clone(),
            },
        );
    plan.credentials.insert(
        FUTURE_REFERENCE.into(),
        Secret::new("offline-future-model-token"),
    );
    future_step(plan);
    source
}

fn prepared(world: &mut World, mode: &str, future: bool) -> (RunPlan, PathBuf, &'static str) {
    let (mut plan, source) = prepare(world, mode);
    if future {
        let source = future_source(world, &mut plan);
        return (plan, source, FUTURE_REFERENCE);
    }
    (plan, source, "copilot-model")
}

async fn scenario(mode: &str, expected: (bool, bool, usize), future: bool) {
    let mut world = World::new(false);
    let (plan, source, reference) = prepared(&mut world, mode, future);
    let paused = Paused::execute(&world, plan, source).await;
    paused.assert_continuation(expected);
    independent_work(&mut world, &paused).await;
    idle_failure(&mut world, &paused, reference).await;
}

#[tokio::test]
async fn clean_bootstrap_defers_missing_auth_without_dispatching_a_factory() {
    scenario("bootstrap-pause", (true, false, 0), false).await;
}

#[tokio::test]
async fn another_saved_factory_reference_does_not_invalidate_the_current_continuation() {
    for (mode, expected) in [
        ("pause", (false, true, 1)),
        ("bootstrap-pause", (true, false, 0)),
    ] {
        scenario(mode, expected, true).await;
    }
}

fn assert_indeterminate(paused: &Paused) {
    let record = Evidence::capture(&paused.directory).record;
    assert_eq!(
        (
            record.can_start(),
            record.can_resume(),
            record.can_clean(),
            record.indeterminate.is_some()
        ),
        (false, false, false, true),
    );
}

#[tokio::test]
async fn indeterminate_history_defers_missing_auth_without_authorizing_resume() {
    let mut world = World::new(false);
    let (plan, source) = prepare(&mut world, "ambiguous");
    let paused = Paused::execute(&world, plan, source).await;
    assert_indeterminate(&paused);
    independent_work(&mut world, &paused).await;
    idle_failure(&mut world, &paused, "copilot-model").await;
}
