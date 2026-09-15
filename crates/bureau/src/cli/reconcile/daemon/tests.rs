mod cold;
mod config;
mod files;
#[path = "../../../../tests/factory_engine/plan.rs"]
mod native_plan;
#[path = "../../../../tests/factory_engine/source.rs"]
mod native_source;
mod world;

use std::collections::BTreeMap;

use bureau::contract::StepOutcome;
use bureau::state::LabelRuleEventKind;

use world::World;

fn assert_only_ordinary_started(world: &World) {
    let ids = world.daemon.active_ids();
    assert_eq!(
        (ids.len(), ids.iter().all(|id| id.starts_with("ordinary-"))),
        (1, true),
    );
}

fn assert_independent_progress(world: &World) {
    let runs: Vec<_> = world
        .records()
        .into_iter()
        .map(|record| (record.snapshot.assignment.name, record.finished.outcome))
        .collect();
    assert_eq!(
        (
            runs,
            world.factory_leases(),
            world.factory_headroom(),
            world.labels(),
            world.label_events()
        ),
        (
            vec![("ordinary".into(), StepOutcome::NoWork)],
            0,
            1,
            vec!["eligible".into()],
            vec![
                LabelRuleEventKind::UpdateStarted,
                LabelRuleEventKind::UpdateApplied
            ],
        ),
    );
}

#[tokio::test]
async fn unavailable_factory_source_does_not_abort_daemon_work_or_label_rules() {
    let mut world = World::new(false);
    world
        .daemon
        .pass()
        .await
        .expect("daemon pass isolates the missing model source");
    assert_only_ordinary_started(&world);
    world.finish().await;
    assert_independent_progress(&world);
    assert_eq!(
        world.errors(),
        BTreeMap::from([(
            "missing-model".into(),
            "credential `missing-model` is unavailable from its declared source".into(),
        )]),
    );
}

#[tokio::test]
async fn failed_model_source_overrides_an_existing_repository_secret() {
    let mut world = World::new(true);
    world
        .daemon
        .pass()
        .await
        .expect("repository value cannot authorize a failed model source");
    assert_only_ordinary_started(&world);
    world.finish().await;
    assert_independent_progress(&world);
}

#[tokio::test]
async fn idle_repeated_daemon_pass_reports_failure_without_repeating_healthy_work() {
    let mut world = World::new(false);
    world.daemon.pass().await.expect("first daemon pass");
    assert_only_ordinary_started(&world);
    world.finish().await;
    let error = world
        .daemon
        .pass()
        .await
        .expect_err("idle pass still reports the missing model credential");
    assert_eq!(
        error.to_string(),
        "local factory model credential `missing-model` was not resolved"
    );
    assert_independent_progress(&world);
}
