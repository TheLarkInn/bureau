use std::collections::BTreeMap;
use std::sync::Arc;

use bureau::config::{Config, Pipeline};
use bureau::engine::{RunPlan, new_run_id};
use bureau::forge::fake::FakeForge;
use bureau::process::Secret;
use serde_json::json;

use super::super::world::World;

fn pipeline() -> Pipeline {
    serde_json::from_value(json!({"name": "ordinary", "steps": [
        {"name": "first", "type": "deterministic", "run": "touch ../PAUSE", "next": "last"},
        {"name": "last", "type": "deterministic", "run": "true", "next": "done"}
    ]}))
    .expect("ordinary pause-at-boundary pipeline")
}

fn credentials(world: &World) -> BTreeMap<String, Secret> {
    let settings = world
        .daemon
        .settings
        .as_ref()
        .expect("explicit fixture settings");
    let token =
        bureau::credential::resolve(settings, "git-main").expect("declared fake repo value");
    BTreeMap::from([("git-main".into(), token)])
}

fn plan(world: &World) -> RunPlan {
    let config = Config::load(&world.root().join("source/.bureau")).expect("fixture config");
    let mut assignment = config.assignments["ordinary"].clone();
    assignment.name = "zzz-recovery".into();
    RunPlan {
        run_id: new_run_id("zzz-recovery").expect("ordinary recovery identity"),
        assignment,
        pipeline: pipeline(),
        roles: config.roles,
        repos: BTreeMap::from([("main".into(), config.repos["main"].clone())]),
        item: serde_json::from_value(
            json!({"external_id":"recovered", "title":"Recovery", "body":"",
            "url":"fake://recovered", "labels":[], "trust":"untrusted"}),
        )
        .expect("ordinary item"),
        forge: Arc::new(FakeForge::default()),
        credentials: credentials(world),
        config_source: None,
        plugin_sources: BTreeMap::new(),
        direct_agents: BTreeMap::new(),
        lease: None,
    }
}

pub(super) async fn paused(world: &World) -> String {
    let mut plan = plan(world);
    let owner = world
        .daemon
        .owner(&plan.snapshot())
        .expect("ordinary original owner");
    assert!(
        owner
            .claim(bureau::supervise::LEASE_TTL)
            .expect("ordinary claim")
    );
    plan.lease = Some(owner.clone());
    let outcome = world.daemon.engine.run(&plan).await;
    assert!(outcome.message.contains("paused"), "{outcome:?}");
    owner.release().expect("release ordinary original owner");
    let directory = world.daemon.engine.runs_dir.join(&plan.run_id);
    std::fs::remove_file(directory.join("PAUSE")).expect("authorize ordinary recovery");
    plan.run_id
}
