use std::collections::BTreeMap;
use std::sync::Arc;

use bureau::config::Config;
use bureau::engine::{Engine, RunOutcome};
use bureau::forge::Forge;
use bureau::reconcile::{Reconciler, Started};
use bureau::runlog::ConfigSource;
use bureau::state::{LeaseOwner, Store};

use super::super::fixture::Fixture;

pub fn store(fixture: &Fixture) -> Arc<Store> {
    Arc::new(Store::open(&fixture.root.join("state.db")).expect("shared scheduler database"))
}

fn config(fixture: &Fixture) -> Config {
    let plan = &fixture.plan;
    Config {
        repos: plan.repos.clone(),
        roles: plan.roles.clone(),
        assignments: BTreeMap::from([(plan.assignment.name.clone(), plan.assignment.clone())]),
        label_rules: BTreeMap::new(),
        pipelines: BTreeMap::from([(plan.pipeline.name.clone(), plan.pipeline.clone())]),
    }
}

pub fn engine(fixture: &Fixture) -> Arc<Engine> {
    Arc::new(Engine::new(
        fixture.root.join("runs"),
        fixture.root.join("cache"),
    ))
}

pub fn reconciler(fixture: &Fixture, forge: Arc<dyn Forge>) -> Arc<Reconciler> {
    Arc::new(Reconciler {
        config: config(fixture),
        state: store(fixture),
        forges: BTreeMap::from([(fixture.plan.assignment.name.clone(), forge)]),
        label_forges: BTreeMap::new(),
        engine: engine(fixture),
        credentials: fixture.plan.credentials.clone(),
        model_credential_errors: BTreeMap::new(),
        config_source: ConfigSource {
            remote: "fixture".into(),
            reference: "main".into(),
            commit: "0000000000000000000000000000000000000000".into(),
        },
        direct_agents: fixture.plan.direct_agents.clone(),
    })
}

pub async fn supervise(fixture: &Fixture) -> RunOutcome {
    let (outcome, projection) =
        bureau::supervise::run(engine(fixture), store(fixture), fixture.plan.clone()).await;
    projection.expect("unfinished factory remains explicitly preserved");
    outcome
}

pub fn recovery_owner(fixture: &Fixture) -> LeaseOwner {
    let owner = LeaseOwner::new(
        store(fixture),
        &fixture.plan.assignment.name,
        "github",
        &fixture.plan.item.external_id,
        &fixture.plan.run_id,
    )
    .expect("same-run recovery owner");
    assert!(
        owner
            .claim(bureau::supervise::LEASE_TTL)
            .expect("recovery claim")
    );
    owner
}

pub fn write_events(fixture: &Fixture, events: &[bureau::runlog::Event]) {
    let lines = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .expect("event JSON")
        .join("\n");
    std::fs::write(
        fixture.directory().join("events.jsonl"),
        format!("{lines}\n"),
    )
    .expect("replace fixture events");
}

pub async fn settle(started: Vec<Started>) -> usize {
    let count = started.len();
    for run in started {
        run.handle.await.expect("unexpected run still joins");
    }
    count
}

async fn pass(reconciler: &Reconciler) -> usize {
    settle(reconciler.reconcile_once().await.expect("pass")).await
}

pub async fn repeated_passes(reconciler: &Reconciler) -> Vec<usize> {
    let mut starts = Vec::new();
    for _ in 0..3 {
        starts.push(pass(reconciler).await);
    }
    starts
}

pub fn directories(fixture: &Fixture) -> Vec<String> {
    let mut directories: Vec<_> = std::fs::read_dir(&fixture.engine.runs_dir)
        .expect("run directories")
        .map(|entry| {
            entry
                .expect("run directory")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    directories.sort();
    directories
}
