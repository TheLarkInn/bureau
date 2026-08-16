//! The e2e world: a bare local "remote" whose default branch fails the
//! check, plus `RunPlan` assembly for the reference pipeline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::adapters::AdapterKind;
use bureau::config::{
    Access, Assignment, ForgeKind, Limits, Pipeline, Repo, Role, StepDef, WorkSource,
};
use bureau::contract::Trust;
use bureau::engine::{Engine, RunPlan, new_run_id};
use bureau::forge::Item;
use bureau::forge::fake::FakeForge;
use bureau::process::Secret;

use super::steps::CHECK;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A temporary directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-e2e-{}-{}-{tag}",
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

/// Runs git with a fixed commit identity, whatever the host config.
fn commit(dir: &Path, args: &[&str]) {
    let mut full = vec!["-c", "user.name=test", "-c", "user.email=test@test"];
    full.extend_from_slice(args);
    git(dir, &full);
}

/// A bare remote and a seed clone; the remote's `main` holds `answer.txt`
/// with 41, so `CHECK` fails until a run fixes it.
fn make_remote(parent: &Path) -> (String, PathBuf) {
    let seed = parent.join("seed");
    git(parent, &["init", "-b", "main", "seed"]);
    std::fs::write(seed.join("answer.txt"), "41\n").expect("seed file");
    git(&seed, &["add", "-A"]);
    commit(&seed, &["commit", "-m", "init"]);
    git(parent, &["init", "--bare", "-b", "main", "remote.git"]);
    let url = parent.join("remote.git").to_string_lossy().into_owned();
    git(&seed, &["push", &url, "main"]);
    (url, seed)
}

/// One test's world: temp dirs, a bare remote, one item on a fake forge.
pub struct Rig {
    /// Temp root, removed on drop.
    pub dir: TestDir,
    /// The forge the item and PRs live on.
    pub forge: Arc<FakeForge>,
    /// The primary repo's registry URL: the bare remote's path.
    pub url: String,
    /// A working clone of the remote, for seeding and merging.
    seed: PathBuf,
}

impl Rig {
    /// A world whose remote's default branch fails `CHECK`.
    pub fn new() -> Self {
        let dir = TestDir::new("pipeline");
        let (url, seed) = make_remote(dir.path());
        Self {
            dir,
            forge: Arc::new(FakeForge::new(vec![item()])),
            url,
            seed,
        }
    }

    /// An engine writing runs and mirrors under this rig's temp dir.
    pub fn engine(&self) -> Engine {
        Engine::new(self.dir.path().join("runs"), self.dir.path().join("cache"))
    }

    /// Lands the run branch on the remote's default branch, as a human
    /// merging the PR would.
    pub fn merge_into_main(&self, branch: &str) {
        git(&self.seed, &["fetch", &self.url, branch]);
        commit(&self.seed, &["merge", "--no-edit", "FETCH_HEAD"]);
        git(&self.seed, &["push", &self.url, "main"]);
    }

    /// A plan running `steps` against this rig's repo and forge.
    pub fn plan(&self, steps: Vec<StepDef>) -> RunPlan {
        RunPlan {
            run_id: new_run_id("fix-failing-test").expect("run id"),
            assignment: assignment(),
            pipeline: Pipeline {
                name: "fix-failing-test".to_owned(),
                steps,
            },
            roles: BTreeMap::from([
                ("implementer".to_owned(), role("implementer")),
                ("reviewer".to_owned(), role("reviewer")),
            ]),
            repos: BTreeMap::from([("main".to_owned(), repo(&self.url))]),
            item: item(),
            forge: self.forge.clone(),
            credentials: BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))]),
            config_source: None,
            plugin_sources: BTreeMap::new(),
            direct_agents: BTreeMap::new(),
            lease: None,
        }
    }
}

/// Both agent roles run offline through the `fake` adapter.
fn role(name: &str) -> Role {
    Role {
        name: name.to_owned(),
        agent: format!("agents/{name}.md"),
        adapter: AdapterKind::Fake,
        permissions: Vec::new(),
        min_trust: Trust::Untrusted,
    }
}

/// The primary repo: pushes go to the local bare remote.
fn repo(url: &str) -> Repo {
    Repo {
        url: url.to_owned(),
        forge: ForgeKind::Github,
        access: Access::Push,
        credential: "git-main".to_owned(),
    }
}

/// Headroom is irrelevant here; the engine enforces none of it.
const fn limits() -> Limits {
    Limits {
        max_concurrent: Some(1),
        max_runs_per_hour: Some(10),
        max_runs_per_day: Some(20),
        max_open_prs: Some(5),
        max_cost_per_day_usd: Some(50.0),
        max_run_hours: None,
    }
}

/// The assignment under test: DESIGN.md section 11's own.
fn assignment() -> Assignment {
    Assignment {
        name: "fix-failing-test".to_owned(),
        work: WorkSource {
            forge: ForgeKind::Github,
            source: "fake".to_owned(),
            filter: "*".to_owned(),
            approval_label: None,
        },
        repos: vec!["main".to_owned()],
        pipeline: "fix-failing-test".to_owned(),
        role: "implementer".to_owned(),
        verify: CHECK.to_owned(),
        branch_prefix: "bureau/fix/".to_owned(),
        limits: limits(),
    }
}

/// The claimed work item the run operates on.
fn item() -> Item {
    Item {
        external_id: "42".to_owned(),
        title: "answer.txt holds 41, the check wants 42".to_owned(),
        body: "Make the check pass.".to_owned(),
        url: "fake://item/42".to_owned(),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}
