use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use bureau::config::{Config, Limits};
use bureau::contract::Trust;
use bureau::engine::Engine;
use bureau::forge::fake::FakeForge;
use bureau::forge::{Error, Forge, Item, Pr, PrRequest, PrStatus};
use bureau::reconcile::Reconciler;
use bureau::runlog::ConfigSource;
use bureau::state::Store;
use tokio::sync::Barrier;

pub const ASSIGNMENT: &str = "work";

const CONFIG: &str = "
repos:
  local: {url: unused, forge: github, access: read, credential: unused}
roles: {}
assignments:
  work:
    name: work
    work:
      forge: github
      source: items
      filter: '*'
      abort_label: 'bureau:failed'
      escalate_label: 'bureau:needs-human'
    repos: [local]
    pipeline: check
    role: unused
    verify: 'true'
    branch_prefix: bureau/
pipelines:
  check:
    name: check
    steps: [{name: check, type: deterministic, run: 'true', next: done}]
";

pub fn item(id: u32) -> Item {
    Item {
        external_id: id.to_string(),
        title: format!("Item {id}"),
        body: format!("Offline item {id}"),
        url: format!("fake://item/{id}"),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

struct Observation {
    forge: FakeForge,
    barrier: Arc<Barrier>,
}

#[async_trait]
impl Forge for Observation {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        if source == "barrier" {
            self.barrier.wait().await;
            return Ok(Vec::new());
        }
        self.forge.query(source, filter).await
    }

    async fn open_prs(&self, repo: &str, prefix: &str) -> Result<Vec<Pr>, Error> {
        self.forge.open_prs(repo, prefix).await
    }

    async fn create_pr(&self, request: &PrRequest) -> Result<Pr, Error> {
        self.forge.create_pr(request).await
    }

    async fn pr_status(&self, repo: &str, number: u64) -> Result<PrStatus, Error> {
        self.forge.pr_status(repo, number).await
    }

    async fn comment(&self, id: &str, body: &str) -> Result<(), Error> {
        self.forge.comment(id, body).await
    }

    async fn set_labels(&self, id: &str, labels: &[String]) -> Result<(), Error> {
        self.forge.set_labels(id, labels).await
    }

    async fn update_labels(
        &self,
        id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error> {
        self.forge.update_labels(id, add, remove).await
    }
}

pub struct TestDir(PathBuf);

impl TestDir {
    pub(super) fn new(seed: u32) -> Self {
        let path = std::env::temp_dir().join(format!("bureau-chaos-{}-{seed}", std::process::id()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        builder.create(&path).expect("private fixture directory");
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("fixture cleanup failed for {}: {error}", self.0.display());
            assert!(std::thread::panicking(), "fixture cleanup must succeed");
        }
    }
}

fn config(root: &Path, limit: u32) -> Config {
    let mut config: Config = serde_yaml_ng::from_str(CONFIG).expect("fixture config");
    config.repos.get_mut("local").expect("local repo").url =
        root.join("unpolled-repo").to_string_lossy().into_owned();
    let assignment = config.assignments.get_mut(ASSIGNMENT).expect("assignment");
    assignment.limits.max_concurrent = Some(limit);
    let mut barrier = assignment.clone();
    "z-observation".clone_into(&mut barrier.name);
    "barrier".clone_into(&mut barrier.work.source);
    config.assignments.insert(barrier.name.clone(), barrier);
    config
}

fn source() -> ConfigSource {
    ConfigSource {
        remote: "offline".to_owned(),
        reference: "main".to_owned(),
        commit: "0000000000000000000000000000000000000000".to_owned(),
    }
}

fn reconciler(root: &Path, forge: &Arc<dyn Forge>, limit: u32) -> Reconciler {
    Reconciler {
        config: config(root, limit),
        state: Arc::new(Store::open(&root.join("state.db")).expect("shared database")),
        forges: BTreeMap::from([
            (ASSIGNMENT.to_owned(), forge.clone()),
            ("z-observation".to_owned(), forge.clone()),
        ]),
        label_forges: BTreeMap::new(),
        engine: Arc::new(Engine::new(root.join("runs"), root.join("cache"))),
        credentials: BTreeMap::new(),
        model_credential_errors: BTreeMap::new(),
        config_source: source(),
        direct_agents: BTreeMap::new(),
    }
}

fn ordered_items(seed: u32, limit: u32) -> Vec<Item> {
    let mut items: Vec<Item> = (0..2 * limit + 2).map(item).collect();
    let rotation = usize::try_from(seed).expect("seed fits usize") % items.len();
    items.rotate_left(rotation);
    items
}

pub struct Fixture {
    pub(super) one: Reconciler,
    pub(super) two: Reconciler,
    pub(super) limit: u32,
    pub(super) barrier: Arc<Barrier>,
    pub(super) first_item: Item,
    directory: TestDir,
}

impl Fixture {
    pub(super) fn new(seed: u32) -> Self {
        let directory = TestDir::new(seed);
        let limit = 1 + seed % 3;
        let items = ordered_items(seed, limit);
        let first_item = items.first().cloned().expect("nonempty work fixture");
        let barrier = Arc::new(Barrier::new(3));
        let forge: Arc<dyn Forge> = Arc::new(Observation {
            forge: FakeForge::new(items),
            barrier: barrier.clone(),
        });
        Self {
            one: reconciler(directory.path(), &forge, limit),
            two: reconciler(directory.path(), &forge, limit),
            limit,
            barrier,
            first_item,
            directory,
        }
    }

    pub(super) fn set_limits(&mut self, limits: &Limits) {
        for reconciler in [&mut self.one, &mut self.two] {
            reconciler
                .config
                .assignments
                .get_mut(ASSIGNMENT)
                .expect("fixture assignment")
                .limits
                .clone_from(limits);
        }
    }

    pub(super) fn database(&self) -> PathBuf {
        self.directory.path().join("state.db")
    }
}
