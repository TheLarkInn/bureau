//! Offline behavior tests for bounded dependency-driven label rules.

#[path = "label_rules/recovery.rs"]
mod recovery;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{
    Access, Config, ForgeKind, LabelRule, LabelRuleCondition, LabelRuleLimits, LabelRuleWork, Repo,
};
use bureau::contract::Trust;
use bureau::engine::Engine;
use bureau::forge::fake::FakeForge;
use bureau::forge::{Dependency, Item, LabelForge};
use bureau::reconcile::Reconciler;
use bureau::runlog::ConfigSource;
use bureau::state::{LabelRuleEventKind, Store};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-label-rule-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn item(id: &str) -> Item {
    Item {
        external_id: format!("TheLarkInn/bureau#{id}"),
        title: format!("Issue {id}"),
        body: String::new(),
        url: format!("https://github.com/TheLarkInn/bureau/issues/{id}"),
        labels: vec!["agent-blocked".to_owned()],
        trust: Trust::Untrusted,
    }
}

fn dependency(id: &str, closed: bool) -> Dependency {
    Dependency {
        external_id: format!("TheLarkInn/bureau#{id}"),
        closed,
    }
}

fn rule(max_updates_per_hour: u32) -> LabelRule {
    LabelRule {
        name: "graduate-unblocked".to_owned(),
        work: LabelRuleWork {
            forge: ForgeKind::Github,
            source: "TheLarkInn/bureau".to_owned(),
            filter: "is:issue is:open label:agent-blocked".to_owned(),
        },
        when: LabelRuleCondition::DependenciesClosed,
        add_labels: vec!["agent-eligible".to_owned()],
        remove_labels: vec!["agent-blocked".to_owned()],
        limits: LabelRuleLimits {
            max_updates_per_hour,
        },
    }
}

struct World {
    _dir: TestDir,
    forge: Arc<FakeForge>,
    store: Arc<Store>,
    reconciler: Reconciler,
}

impl World {
    fn new(ids: &[&str], max_updates_per_hour: u32) -> Self {
        let dir = TestDir::new();
        let rule = rule(max_updates_per_hour);
        let rule_name = rule.name.clone();
        let forge = Arc::new(FakeForge::new(ids.iter().map(|id| item(id)).collect()));
        let store = Arc::new(Store::open_in_memory().expect("state store"));
        let config = config(rule);
        let reconciler = Reconciler {
            config,
            state: store.clone(),
            forges: BTreeMap::new(),
            label_forges: BTreeMap::from([(rule_name, forge.clone() as Arc<dyn LabelForge>)]),
            engine: Arc::new(Engine::new(dir.0.join("runs"), dir.0.join("cache"))),
            credentials: BTreeMap::new(),
            config_source: config_source(),
            direct_agents: BTreeMap::new(),
        };
        Self {
            _dir: dir,
            forge,
            store,
            reconciler,
        }
    }

    async fn pass(&self) -> Result<(), bureau::reconcile::Error> {
        let started = self.reconciler.reconcile_once().await?;
        assert!(started.is_empty());
        Ok(())
    }

    fn events(&self) -> Vec<bureau::state::LabelRuleEvent> {
        self.store
            .label_rule_events("graduate-unblocked")
            .expect("audit events")
    }
}

fn config(rule: LabelRule) -> Config {
    let repo = Repo {
        url: "https://github.com/TheLarkInn/bureau".to_owned(),
        forge: ForgeKind::Github,
        access: Access::Read,
        credential: "github".to_owned(),
    };
    Config {
        repos: BTreeMap::from([("bureau".to_owned(), repo)]),
        roles: BTreeMap::new(),
        assignments: BTreeMap::new(),
        label_rules: BTreeMap::from([(rule.name.clone(), rule)]),
        pipelines: BTreeMap::new(),
    }
}

fn config_source() -> ConfigSource {
    ConfigSource {
        remote: "fixture".to_owned(),
        reference: "main".to_owned(),
        commit: "0000000000000000000000000000000000000000".to_owned(),
    }
}

fn interrupted_audit(id: &str) -> bureau::state::LabelRuleAudit {
    bureau::state::LabelRuleAudit {
        attempt_id: format!("interrupted-{id}"),
        rule: "graduate-unblocked".to_owned(),
        source: "github.com:443/thelarkinn/bureau".to_owned(),
        item: item(id).external_id,
        add_labels: vec!["agent-eligible".to_owned()],
        remove_labels: vec!["agent-blocked".to_owned()],
        dependency_count: 1,
        closed_dependency_count: 1,
    }
}

#[tokio::test]
async fn closed_dependencies_graduate_and_write_audit_events() {
    let world = World::new(&["42"], 20);
    world
        .forge
        .set_dependencies(&item("42").external_id, vec![dependency("1", true)]);
    world.pass().await.expect("label pass");
    let events = world.events();
    let observed = (
        world.forge.labels_of(&item("42").external_id),
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        events[0]
            .message
            .contains("unblocked; graduating blocked work item"),
    );
    assert_eq!(
        observed,
        (
            vec!["agent-eligible".to_owned()],
            vec![
                LabelRuleEventKind::UpdateStarted,
                LabelRuleEventKind::UpdateApplied,
            ],
            true,
        )
    );
}

#[tokio::test]
async fn waiting_item_is_reconsidered_after_its_dependency_closes() {
    let world = World::new(&["42"], 20);
    let id = item("42").external_id;
    world
        .forge
        .set_dependencies(&id, vec![dependency("1", false)]);
    world.pass().await.expect("waiting pass");
    world
        .forge
        .set_dependencies(&id, vec![dependency("1", true)]);
    world.pass().await.expect("ready pass");
    let events = world.events();
    assert_eq!(
        (world.forge.labels_of(&id), events.len()),
        (vec!["agent-eligible".to_owned()], 2)
    );
}

#[tokio::test]
async fn hourly_limit_bounds_attempted_mutations() {
    let world = World::new(&["1", "2"], 1);
    world.pass().await.expect("bounded pass");
    let promoted = ["1", "2"]
        .iter()
        .filter(|id| {
            world
                .forge
                .labels_of(&item(id).external_id)
                .contains(&"agent-eligible".to_owned())
        })
        .count();
    let events = world.events();
    assert_eq!((promoted, events.len()), (1, 2));
}

#[tokio::test]
async fn failed_update_is_durable_and_retryable() {
    let world = World::new(&["42"], 20);
    world.forge.fail_label_updates("permission denied");
    let error = world.pass().await.expect_err("update fails");
    let events = world.events();
    let observed = (
        error.to_string().contains("permission denied"),
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        events[1].message.contains("next reconcile pass will retry"),
    );
    assert_eq!(
        observed,
        (
            true,
            vec![
                LabelRuleEventKind::UpdateStarted,
                LabelRuleEventKind::UpdateFailed,
            ],
            true,
        )
    );
}

#[tokio::test]
async fn rate_limit_stops_remaining_candidates() {
    let world = World::new(&["1", "2"], 20);
    world.forge.rate_limit_next_label_update();
    let error = world.pass().await.expect_err("rate limit");
    let labels = ["1", "2"].map(|id| world.forge.labels_of(&item(id).external_id));
    assert_eq!(
        (error.to_string().contains("rate limit"), labels),
        (
            true,
            [
                vec!["agent-blocked".to_owned()],
                vec!["agent-blocked".to_owned()],
            ],
        )
    );
}
