//! Support for the run-branch continuity behavior ports: a local git
//! remote, the fake adapter and forge, and the implement -> review ->
//! local-ci pipeline the continuity scenarios run. Offline only.

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
            "bureau-behavior-{}-{}-{tag}",
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

/// A bare remote holding one committed file.
fn make_remote(parent: &Path, name: &str) -> String {
    let work = parent.join(name);
    git(parent, &["init", "-b", "main", name]);
    std::fs::write(work.join("file.txt"), "start\n").expect("seed file");
    git(&work, &["add", "-A"]);
    let identity = ["-c", "user.name=test", "-c", "user.email=test@test"];
    git(
        &work,
        &[&identity[..], &["commit", "-m", "init"][..]].concat(),
    );
    let bare = format!("{name}.git");
    git(parent, &["clone", "--bare", name, &bare]);
    parent.join(bare).to_string_lossy().into_owned()
}

/// Writes a `fake` adapter fixture whose stdout is the step result.
pub fn fixture(dir: &Path, name: &str, outcome: StepOutcome, message: &str) -> String {
    let result = StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    };
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
fn step(name: &str, kind: StepKind) -> StepDef {
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

/// A deterministic step routing success to `next`.
fn det_step(name: &str, run: &str, next: &str) -> StepDef {
    let mut step = step(name, StepKind::Deterministic);
    step.run = Some(run.to_owned());
    step.next = Some(next.to_owned());
    step
}

/// An agent step replaying `fixture`; failures escalate.
fn agent_step(name: &str, role: &str, fixture: &str, next: &str) -> StepDef {
    let mut step = step(name, StepKind::Agent);
    step.role = Some(role.to_owned());
    step.fixture = Some(fixture.to_owned());
    step.next = Some(next.to_owned());
    step.on_failure = Some("escalate".to_owned());
    step.on_blocked = Some("escalate".to_owned());
    step
}

/// The continuity pipeline: implement (agent) materializes its change
/// in `change` (deterministic), review (agent) passes, the verdict
/// routes to local-ci, which passes only when the implement stage's
/// committed file is visible in its worktree — the run-branch
/// continuity property (goober issue #133's port).
pub fn continuity_steps(implement: &str, review: &str) -> Vec<StepDef> {
    let mut verdict = step("verdict", StepKind::Decision);
    verdict.over = Some("review".to_owned());
    verdict.on = BTreeMap::from([
        ("success".to_owned(), "local-ci".to_owned()),
        ("failure".to_owned(), "abort".to_owned()),
        ("blocked".to_owned(), "escalate".to_owned()),
        ("no-work".to_owned(), "abort".to_owned()),
    ]);
    vec![
        agent_step("implement", "implementer", implement, "change"),
        det_step("change", "echo implemented > IMPLEMENTED", "review"),
        agent_step("review", "reviewer", review, "verdict"),
        verdict,
        det_step(
            "local-ci",
            "test -f IMPLEMENTED && git diff --quiet HEAD -- IMPLEMENTED",
            "done",
        ),
    ]
}

/// One test's world: temp dirs, one fake forge, bare remotes on demand.
pub struct Rig {
    pub dir: TestDir,
    pub forge: Arc<FakeForge>,
}

fn role(name: &str) -> Role {
    Role {
        name: name.to_owned(),
        agent: format!("agents/{name}.md"),
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
        max_concurrent: Some(2),
        max_runs_per_hour: Some(10),
        max_runs_per_day: Some(20),
        max_open_prs: Some(5),
        max_cost_per_day_usd: Some(50.0),
        max_run_hours: None,
    }
}

fn assignment() -> Assignment {
    Assignment {
        name: "fix-failing-test".to_owned(),
        work: WorkSource {
            forge: ForgeKind::Github,
            source: "fake".to_owned(),
            filter: "*".to_owned(),
            approval_label: None,
            abort_label: "bureau:failed".to_owned(),
            escalate_label: "bureau:needs-human".to_owned(),
        },
        repos: vec!["main".to_owned()],
        pipeline: "continuity".to_owned(),
        role: "implementer".to_owned(),
        verify: "test -f IMPLEMENTED".to_owned(),
        branch_prefix: "bureau/fix/".to_owned(),
        limits: limits(),
    }
}

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

impl Rig {
    /// A world whose forge holds one item.
    pub fn new() -> Self {
        Self::with_items(vec![item("101")])
    }

    /// A world whose forge holds the given items.
    pub fn with_items(items: Vec<Item>) -> Self {
        let dir = TestDir::new("continuity");
        Self {
            dir,
            forge: Arc::new(FakeForge::new(items)),
        }
    }

    /// Adds a bare remote named `name` and returns its URL.
    pub fn add_remote(&self, name: &str) -> String {
        make_remote(self.dir.path(), name)
    }

    /// One engine over this rig's shared cache — the same cache every
    /// concurrent run draws its worktrees from.
    pub fn engine(&self) -> Engine {
        Engine::new(self.dir.path().join("runs"), self.dir.path().join("cache"))
    }

    /// A plan running `steps` against `url`, claiming `item_id`.
    pub fn plan(&self, url: &str, item_id: &str, steps: Vec<StepDef>) -> RunPlan {
        RunPlan {
            run_id: new_run_id("fix-failing-test").expect("run id"),
            assignment: assignment(),
            pipeline: Pipeline {
                name: "continuity".to_owned(),
                steps,
            },
            roles: BTreeMap::from([
                ("implementer".to_owned(), role("implementer")),
                ("reviewer".to_owned(), role("reviewer")),
            ]),
            repos: BTreeMap::from([("main".to_owned(), repo(url))]),
            item: item(item_id),
            forge: self.forge.clone(),
            credentials: BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))]),
            config_source: None,
            plugin_sources: BTreeMap::new(),
            direct_agents: BTreeMap::new(),
            lease: None,
        }
    }
}
