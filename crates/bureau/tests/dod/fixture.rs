//! Fixtures for the section 13 definition-of-done proofs: a local git repo, a fake
//! forge, one run plan, and two reconcilers sharing a single state DB
//! file. Offline only (DESIGN.md section 12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{
    Access, Assignment, Config, ForgeKind, Limits, Pipeline, Repo, StepDef, StepKind, WorkSource,
};
use bureau::contract::Trust;
use bureau::engine::{Engine, RunPlan, new_run_id};
use bureau::forge::fake::FakeForge;
use bureau::forge::{Forge, Item};
use bureau::process::Secret;
use bureau::reconcile::{Reconciler, Started};
use bureau::runlog::ConfigSource;
use bureau::state::Store;

/// The one assignment every fixture uses.
const ASSIGNMENT: &str = "fix-tests";

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A temporary directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-dod-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.0
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

/// A local repo with one committed file; its path serves as the URL,
/// which clone and push both accept offline.
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
fn item(id: &str) -> Item {
    Item {
        external_id: id.to_owned(),
        title: format!("Fix {id}"),
        body: format!("{id} is broken"),
        url: format!("fake://item/{id}"),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

/// A deterministic step running `run`, routing success to `next`.
pub fn det_step(name: &str, run: &str, next: Option<&str>) -> StepDef {
    StepDef {
        name: name.to_owned(),
        kind: StepKind::Deterministic,
        run: Some(run.to_owned()),
        role: None,
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        steps: Vec::new(),
        completion: None,
        max_concurrent: None,
        next: next.map(str::to_owned),
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// Budget limits that never gate a test.
const fn generous() -> Limits {
    Limits {
        max_concurrent: Some(5),
        max_runs_per_hour: Some(10),
        max_runs_per_day: Some(20),
        max_open_prs: Some(5),
        max_cost_per_day_usd: Some(50.0),
        max_run_hours: None,
    }
}

fn assignment(limits: Limits) -> Assignment {
    Assignment {
        name: ASSIGNMENT.to_owned(),
        work: WorkSource {
            forge: ForgeKind::Github,
            source: "fake".to_owned(),
            filter: "*".to_owned(),
            approval_label: None,
            abort_label: "bureau:failed".to_owned(),
            escalate_label: "bureau:needs-human".to_owned(),
        },
        repos: vec!["main".to_owned()],
        pipeline: "fix".to_owned(),
        role: "worker".to_owned(),
        verify: "true".to_owned(),
        branch_prefix: "bureau/".to_owned(),
        limits,
    }
}

/// A registry repo entry: the URL, its forge, push access.
fn repo(url: &str) -> Repo {
    Repo {
        url: url.to_owned(),
        forge: ForgeKind::Github,
        access: Access::Push,
        credential: "git-main".to_owned(),
    }
}

fn config(url: &str, limits: Limits) -> Config {
    let steps = vec![det_step("edit", "echo changed >> file.txt", Some("done"))];
    let pipeline = Pipeline {
        name: "fix".to_owned(),
        steps,
    };
    Config {
        repos: BTreeMap::from([("main".to_owned(), repo(url))]),
        roles: BTreeMap::new(),
        assignments: BTreeMap::from([(ASSIGNMENT.to_owned(), assignment(limits))]),
        label_rules: BTreeMap::new(),
        pipelines: BTreeMap::from([("fix".to_owned(), pipeline)]),
    }
}

/// One resume test's world: temp dirs, a local repo, one item.
pub struct Rig {
    /// The temp dir holding `runs/`, `cache/`, and the seed repo.
    pub dir: TestDir,
    forge: Arc<FakeForge>,
    url: String,
}

impl Rig {
    pub fn new() -> Self {
        let dir = TestDir::new("resume");
        let url = make_repo(dir.path());
        Self {
            dir,
            forge: Arc::new(FakeForge::new(vec![item("42")])),
            url,
        }
    }

    pub fn engine(&self) -> Engine {
        Engine::new(self.dir.path().join("runs"), self.dir.path().join("cache"))
    }

    /// A plan whose git credential is `secret`. The scrubber's holdback
    /// is `secret.len() - 1`, so a 1-byte secret keeps the log tear-free
    /// while a longer one exercises the torn-tail shape.
    pub fn plan(&self, secret: &str, steps: Vec<StepDef>) -> RunPlan {
        RunPlan {
            run_id: new_run_id(ASSIGNMENT).expect("run id"),
            assignment: assignment(generous()),
            pipeline: Pipeline {
                name: "fix".to_owned(),
                steps,
            },
            roles: BTreeMap::new(),
            repos: BTreeMap::from([("main".to_owned(), repo(&self.url))]),
            item: item("42"),
            forge: self.forge.clone(),
            credentials: BTreeMap::from([("git-main".to_owned(), Secret::new(secret))]),
            config_source: None,
            plugin_sources: BTreeMap::new(),
            direct_agents: BTreeMap::new(),
            lease: None,
        }
    }
}

/// One daemon process: its own store connection to the shared DB file.
fn daemon(config: &Config, db: &Path, root: &Path, forge: &Arc<FakeForge>) -> Reconciler {
    Reconciler {
        config: config.clone(),
        state: Arc::new(Store::open(db).expect("store opens")),
        forges: BTreeMap::from([(ASSIGNMENT.to_owned(), forge.clone() as Arc<dyn Forge>)]),
        label_forges: BTreeMap::new(),
        engine: Arc::new(Engine::new(root.join("runs"), root.join("cache"))),
        credentials: BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))]),
        config_source: ConfigSource {
            remote: "fixture".to_owned(),
            reference: "main".to_owned(),
            commit: "0000000000000000000000000000000000000000".to_owned(),
        },
        direct_agents: BTreeMap::new(),
    }
}

/// Two daemons with identical config arbitrating through one DB file.
pub struct Daemons {
    left: Arc<Reconciler>,
    right: Arc<Reconciler>,
    forge: Arc<FakeForge>,
    _dir: TestDir,
}

impl Daemons {
    pub fn new() -> Self {
        let dir = TestDir::new("daemons");
        let forge = Arc::new(FakeForge::new(vec![item("1")]));
        let config = config(&make_repo(dir.path()), generous());
        let db = dir.path().join("state.db");
        Self {
            left: Arc::new(daemon(&config, &db, dir.path(), &forge)),
            right: Arc::new(daemon(&config, &db, dir.path(), &forge)),
            forge,
            _dir: dir,
        }
    }

    /// Both daemons reconcile at once, spawned as concurrent tasks.
    pub async fn pass_both(&self) -> (Vec<Started>, Vec<Started>) {
        let (left, right) = (self.left.clone(), self.right.clone());
        let a = tokio::spawn(async move { left.reconcile_once().await });
        let b = tokio::spawn(async move { right.reconcile_once().await });
        let (a, b) = tokio::join!(a, b);
        let left = a.expect("left joins").expect("left pass");
        let right = b.expect("right joins").expect("right pass");
        (left, right)
    }

    /// How many live leases the shared file records.
    pub fn live_leases(&self) -> usize {
        self.left
            .state
            .active(ASSIGNMENT)
            .expect("active leases")
            .len()
    }

    /// Open PRs the assignment's observation would see.
    pub async fn observed_prs(&self) -> usize {
        let url = &self.left.config.repos.get("main").expect("main repo").url;
        self.forge
            .open_prs(url, "bureau/")
            .await
            .expect("open_prs")
            .len()
    }

    pub async fn join_runs(left: Vec<Started>, right: Vec<Started>) {
        for run in left.into_iter().chain(right) {
            run.handle.await.expect("run joins");
        }
    }
}
