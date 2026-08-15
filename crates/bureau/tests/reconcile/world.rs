//! Fixtures for reconcile tests: a local git repo, the fake forge, an
//! in-memory store, and `Reconciler` assembly. Offline only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bureau::config::{
    Access, Assignment, Config, ForgeKind, Limits, Pipeline, Repo, StepDef, StepKind, WorkSource,
};
use bureau::contract::Trust;
use bureau::engine::Engine;
use bureau::forge::fake::FakeForge;
use bureau::forge::{Forge, Item, PrRequest};
use bureau::process::Secret;
use bureau::reconcile::{Reconciler, Started};
use bureau::state::Store;

/// The one assignment every fixture uses.
pub const ASSIGNMENT: &str = "fix-tests";

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// The tests' clock: real millis since the Unix epoch.
fn test_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A temporary directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-reconcile-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs git for fixture setup, panicking on failure.
fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

/// A local repo with one committed file; its path serves as the URL.
fn make_repo(parent: &Path) -> String {
    let dir = parent.join("repo");
    std::fs::create_dir_all(&dir).expect("repo dir");
    git(&dir, &["init", "-b", "main"]);
    std::fs::write(dir.join("file.txt"), "start\n").expect("seed file");
    git(&dir, &["add", "-A"]);
    let commit = [
        "-c",
        "user.name=test",
        "-c",
        "user.email=test@test",
        "commit",
        "-m",
        "init",
    ];
    git(&dir, &commit);
    dir.to_string_lossy().into_owned()
}

/// A work item with id-derived content.
pub fn item(id: &str) -> Item {
    Item {
        external_id: id.to_owned(),
        title: format!("Fix {id}"),
        body: format!("{id} is broken"),
        url: format!("fake://item/{id}"),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

/// A deterministic step running `run`, then the `done` terminal.
fn det_step(run: &str) -> StepDef {
    StepDef {
        name: "check".to_owned(),
        kind: StepKind::Deterministic,
        run: Some(run.to_owned()),
        role: None,
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        next: Some("done".to_owned()),
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// Budget limits that never constrain a test unless overridden.
pub const fn generous() -> Limits {
    Limits {
        max_concurrent: 5,
        max_runs_per_hour: 10,
        max_runs_per_day: 20,
        max_open_prs: 5,
        max_cost_per_day_usd: 50.0,
    }
}

/// The fixture assignment: one repo, the `fix` pipeline.
fn assignment(limits: Limits) -> Assignment {
    Assignment {
        name: ASSIGNMENT.to_owned(),
        work: WorkSource {
            forge: ForgeKind::Github,
            source: "fake".to_owned(),
            filter: "*".to_owned(),
        },
        repos: vec!["main".to_owned()],
        pipeline: "fix".to_owned(),
        role: "worker".to_owned(),
        verify: "true".to_owned(),
        branch_prefix: "bureau/".to_owned(),
        limits,
    }
}

/// The fixture config: one repo, no roles (deterministic steps only).
fn config(repo: Repo, run: &str, limits: Limits) -> Config {
    let pipeline = Pipeline {
        name: "fix".to_owned(),
        steps: vec![det_step(run)],
    };
    Config {
        repos: BTreeMap::from([("main".to_owned(), repo)]),
        roles: BTreeMap::new(),
        assignments: BTreeMap::from([(ASSIGNMENT.to_owned(), assignment(limits))]),
        pipelines: BTreeMap::from([("fix".to_owned(), pipeline)]),
    }
}

/// The fixture repo: a local git URL, GitHub kind, push access.
fn repo(url: String) -> Repo {
    Repo {
        url,
        forge: ForgeKind::Github,
        access: Access::Push,
        credential: "git-main".to_owned(),
    }
}

/// The reconciler's registry credentials: `git-main` resolves so pushes work.
fn credentials() -> BTreeMap<String, Secret> {
    BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))])
}

/// One test's world: temp dirs, a local repo, a fake forge, the store.
pub struct World {
    dir: TestDir,
    /// The durable store the reconciler claims against.
    pub store: Arc<Store>,
    forge: Arc<FakeForge>,
    /// The reconciler under test.
    pub reconciler: Arc<Reconciler>,
}

impl World {
    /// A world whose pipeline runs `run` and whose budget is `limits`.
    pub fn new(ids: &[&str], run: &str, limits: Limits) -> Self {
        let dir = TestDir::new();
        let items = ids.iter().map(|id| item(id)).collect();
        let forge = Arc::new(FakeForge::new(items));
        let store = Arc::new(Store::open_in_memory(test_clock).expect("in-memory store"));
        let (runs, cache) = (dir.0.join("runs"), dir.0.join("cache"));
        let reconciler = Arc::new(Reconciler {
            config: config(repo(make_repo(&dir.0)), run, limits),
            state: store.clone(),
            forges: vec![(ForgeKind::Github, forge.clone() as Arc<dyn Forge>)],
            engine: Arc::new(Engine::new(runs, cache, test_clock)),
            credentials: credentials(),
            daemon_env: BTreeMap::new(),
            clock: test_clock,
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
            .started
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

    /// Pre-claims an item, as a competing daemon would.
    pub fn claim(&self, id: &str) {
        let ttl = Duration::from_secs(3600);
        let won = self
            .store
            .try_claim(ASSIGNMENT, "github", id, ttl)
            .expect("claim");
        assert!(won, "pre-claim must win");
    }

    /// An open PR for an item, as a finished run would leave behind.
    pub async fn open_pr(&self, id: &str) {
        let request = PrRequest {
            repo: "main".to_owned(),
            branch: format!("bureau/fix-{id}"),
            base: "main".to_owned(),
            title: format!("Fix {id}"),
            body: String::new(),
            item_id: Some(id.to_owned()),
        };
        self.forge.create_pr(&request).await.expect("create_pr");
    }

    /// Open PRs the assignment's observation would see.
    pub async fn observed_prs(&self) -> usize {
        self.forge
            .open_prs("main", "bureau/")
            .await
            .expect("open_prs")
            .len()
    }

    /// How many run directories exist under `runs/`.
    pub fn run_dirs(&self) -> usize {
        std::fs::read_dir(self.dir.0.join("runs")).map_or(0, Iterator::count)
    }
}
