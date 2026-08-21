//! Fixtures for engine tests: a local git repo, the fake forge and
//! adapter, and `RunPlan` assembly. Offline only (DESIGN.md section 12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::adapters::AdapterKind;
use bureau::adapters::fake::{Chunk, Stream, Transcript};
use bureau::config::{
    Access, Assignment, ForgeKind, Limits, Pipeline, Repo, Role, StepDef, StepKind, WorkSource,
};
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};
use bureau::engine::{Engine, RunPlan, new_run_id};
use bureau::forge::Item;
use bureau::forge::fake::FakeForge;
use bureau::process::Secret;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A temporary directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

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

/// A local repo with one committed file; the returned URL is its path,
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

/// Writes a `fake` adapter fixture whose stdout is the step result.
pub fn fixture(dir: &Path, name: &str, result: &StepResult) -> String {
    let mut data = String::from_utf8(result.to_json().expect("result serializes")).expect("utf8");
    data.push('\n');
    let transcript = Transcript {
        schema: SCHEMA_VERSION.to_owned(),
        chunks: vec![Chunk {
            delay_ms: 0,
            stream: Stream::Stdout,
            data,
        }],
        exit_code: 0,
        usage: bureau::adapters::Usage::zero("fake"),
    };
    let path = dir.join(name);
    transcript.save(&path).expect("fixture saves");
    path.to_string_lossy().into_owned()
}

/// A step with every optional field unset.
pub fn step(name: &str, kind: StepKind) -> StepDef {
    StepDef {
        name: name.to_owned(),
        kind,
        run: None,
        role: None,
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        steps: Vec::new(),
        completion: None,
        max_concurrent: None,
        next: None,
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// A deterministic step running `run` with the given success edge.
pub fn det_step(name: &str, run: &str, next: Option<&str>) -> StepDef {
    let mut step = step(name, StepKind::Deterministic);
    step.run = Some(run.to_owned());
    step.next = next.map(str::to_owned);
    step
}

/// An agent step replaying `fixture` through the `fake` adapter.
pub fn agent_step(name: &str, fixture: &str, next: Option<&str>) -> StepDef {
    let mut step = step(name, StepKind::Agent);
    step.role = Some("worker".to_owned());
    step.fixture = Some(fixture.to_owned());
    step.next = next.map(str::to_owned);
    step
}

/// A decision step on `over` that retries `over` on failure.
pub fn decision_step(name: &str, over: &str) -> StepDef {
    let mut step = step(name, StepKind::Decision);
    step.over = Some(over.to_owned());
    step.on = BTreeMap::from([
        ("success".to_owned(), "done".to_owned()),
        ("failure".to_owned(), over.to_owned()),
        ("blocked".to_owned(), "escalate".to_owned()),
        ("no-work".to_owned(), "done".to_owned()),
    ]);
    step
}

/// A step result for fixtures.
pub fn result(outcome: StepOutcome, message: &str) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    }
}

fn role() -> Role {
    Role {
        name: "worker".to_owned(),
        agent: "agents/worker.md".to_owned(),
        adapter: AdapterKind::Fake,
        permissions: Vec::new(),
        min_trust: Trust::Untrusted,
    }
}

fn repo(url: &str) -> Repo {
    Repo {
        url: url.to_owned(),
        forge: ForgeKind::Github,
        access: Access::Push,
        credential: "git-main".to_owned(),
    }
}

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

fn assignment() -> Assignment {
    Assignment {
        name: "fix-tests".to_owned(),
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
        limits: limits(),
    }
}

fn item() -> Item {
    Item {
        external_id: "42".to_owned(),
        title: "Fix the thing".to_owned(),
        body: "It is broken.".to_owned(),
        url: "fake://item/42".to_owned(),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

/// One test's world: temp dirs, a local repo, one item on a fake forge.
pub struct Rig {
    pub dir: TestDir,
    pub forge: Arc<FakeForge>,
    /// The primary repo's registry URL: the local remote's path.
    pub url: String,
}

impl Rig {
    pub fn new() -> Self {
        let dir = TestDir::new("engine");
        let url = make_repo(dir.path());
        Self {
            dir,
            forge: Arc::new(FakeForge::new(vec![item()])),
            url,
        }
    }

    pub fn engine(&self) -> Engine {
        Engine::new(self.dir.path().join("runs"), self.dir.path().join("cache"))
    }

    /// A plan for `steps`; the repo credential resolves so pushes work.
    pub fn plan(&self, steps: Vec<StepDef>) -> RunPlan {
        RunPlan {
            run_id: new_run_id("fix-tests").expect("run id"),
            assignment: assignment(),
            pipeline: Pipeline {
                name: "fix".to_owned(),
                steps,
            },
            roles: BTreeMap::from([("worker".to_owned(), role())]),
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
