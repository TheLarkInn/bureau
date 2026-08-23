//! Assembled worlds for the section 13 tests: the reconcile-level
//! [`World`] and the engine-level [`EngineRig`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use bureau::config::{Limits, Pipeline};
use bureau::engine::{Engine, RunOutcome, RunPlan, new_run_id};
use bureau::forge::Forge;
use bureau::forge::fake::FakeForge;
use bureau::process::Secret;
use bureau::reconcile::{Reconciler, Started};
use bureau::runlog::ConfigSource;
use bureau::state::Store;

use crate::fixtures::{self, ASSIGNMENT, TestDir};

/// One reconcile test's world: temp dirs, a local repo, a fake forge.
pub struct World {
    dir: TestDir,
    /// The durable store the reconciler claims against.
    pub store: Arc<Store>,
    forge: Arc<FakeForge>,
    /// The reconciler under test.
    pub reconciler: Arc<Reconciler>,
}

fn config_source() -> ConfigSource {
    ConfigSource {
        remote: "fixture".to_owned(),
        reference: "main".to_owned(),
        commit: "0000000000000000000000000000000000000000".to_owned(),
    }
}

impl World {
    /// A world whose pipeline runs `run` and whose budget is `limits`.
    pub fn new(ids: &[&str], run: &str, limits: Limits) -> Self {
        let dir = TestDir::new("reconcile");
        let url = fixtures::make_repo(dir.path());
        let items = ids.iter().map(|id| fixtures::item(id)).collect();
        let forge = Arc::new(FakeForge::new(items));
        let store = Arc::new(Store::open_in_memory().expect("in-memory store"));
        let reconciler = Arc::new(Reconciler {
            config: fixtures::config(&url, run, limits),
            state: store.clone(),
            forges: BTreeMap::from([(ASSIGNMENT.to_owned(), forge.clone() as Arc<dyn Forge>)]),
            label_forges: BTreeMap::new(),
            engine: Arc::new(Engine::new(
                dir.path().join("runs"),
                dir.path().join("cache"),
            )),
            credentials: credentials(),
            identities: BTreeMap::new(),
            config_source: config_source(),
            direct_agents: BTreeMap::new(),
        });
        Self {
            dir,
            store,
            forge,
            reconciler,
        }
    }

    /// One reconcile pass, unwrapping the pass-level error.
    pub async fn pass(&self) -> Vec<Started> {
        self.reconciler
            .reconcile_once()
            .await
            .expect("pass succeeds")
    }

    /// The item ids the assignment holds live leases on, sorted.
    pub fn leased(&self) -> Vec<String> {
        let leases = self.store.active(ASSIGNMENT).expect("active leases");
        let mut ids: Vec<String> = leases
            .iter()
            .map(|lease| lease.external_id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Open PRs the assignment's observation would see.
    pub async fn observed_prs(&self) -> usize {
        let url = &self
            .reconciler
            .config
            .repos
            .get("main")
            .expect("main repo")
            .url;
        self.forge
            .open_prs(url, "bureau/")
            .await
            .expect("open_prs")
            .len()
    }

    /// How many run directories exist under `runs/`.
    pub fn run_dirs(&self) -> usize {
        std::fs::read_dir(self.dir.path().join("runs")).map_or(0, Iterator::count)
    }
}

/// The reconciler's registry credentials: `git-main` resolves so pushes work.
fn credentials() -> BTreeMap<String, Secret> {
    BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))])
}

/// One engine test's rig: temp dirs, a local repo, one item on a forge.
pub struct EngineRig {
    dir: TestDir,
    forge: Arc<FakeForge>,
    url: String,
}

impl EngineRig {
    /// A rig with item 42 on the forge.
    pub fn new() -> Self {
        let dir = TestDir::new("engine");
        let url = fixtures::make_repo(dir.path());
        Self {
            dir,
            forge: Arc::new(FakeForge::new(vec![fixtures::item("42")])),
            url,
        }
    }

    /// Runs the one-step pipeline `run` under `credentials`.
    pub async fn run(&self, run: &str, credentials: BTreeMap<String, Secret>) -> RunOutcome {
        let engine = Engine::new(self.dir.path().join("runs"), self.dir.path().join("cache"));
        engine.run(&self.plan(run, credentials)).await
    }

    /// The directory holding run directories.
    pub fn runs(&self) -> PathBuf {
        self.dir.path().join("runs")
    }

    /// The run's plan: the `fix` pipeline's one step, this rig's repo.
    fn plan(&self, run: &str, credentials: BTreeMap<String, Secret>) -> RunPlan {
        RunPlan {
            run_id: new_run_id(ASSIGNMENT).expect("run id"),
            assignment: fixtures::assignment(fixtures::generous()),
            pipeline: Pipeline {
                name: "fix".to_owned(),
                steps: vec![fixtures::det_step(run)],
            },
            roles: BTreeMap::new(),
            repos: BTreeMap::from([("main".to_owned(), fixtures::repo(&self.url))]),
            item: fixtures::item("42"),
            forge: self.forge.clone(),
            credentials,
            identities: BTreeMap::new(),
            config_source: None,
            plugin_sources: BTreeMap::new(),
            direct_agents: BTreeMap::new(),
            lease: None,
        }
    }
}
