//! Support for the reference-stack behavior ports: a seeded local git
//! remote, the fake forge, the change -> verify pipeline, and the
//! opt-in environment gate mirroring goober's `testdep.RequireEnv`.
//! Offline by default; the gated legs fail loud when opted in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{
    Access, Assignment, ForgeKind, Limits, Pipeline, Repo, StepDef, StepKind, WorkSource,
};
use bureau::contract::Trust;
use bureau::engine::{Engine, RunPlan, new_run_id};
use bureau::forge::Item;
use bureau::forge::fake::FakeForge;
use bureau::process::Secret;
use bureau::runlog::{self, Event, EventKind};

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

/// A bare remote seeded with `files`, so the verify step's real
/// toolchain has a project to run against.
fn make_remote(parent: &Path, files: &[(&str, &str)]) -> String {
    let work = parent.join("work");
    git(parent, &["init", "-b", "main", "work"]);
    for (name, text) in files {
        let path = work.join(name);
        std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
        std::fs::write(path, text).expect("write project file");
    }
    git(&work, &["add", "-A"]);
    let identity = ["-c", "user.name=test", "-c", "user.email=test@test"];
    git(
        &work,
        &[&identity[..], &["commit", "-m", "seed"][..]].concat(),
    );
    git(parent, &["clone", "--bare", "work", "remote.git"]);
    parent.join("remote.git").to_string_lossy().into_owned()
}

/// Whether this opt-in leg runs: unset means pass quietly (the offline
/// gate); set means the host promised the toolchain and a missing one
/// is a real misconfiguration — fail loud, exactly as goober's
/// `RequireEnv` + `Require` pair does.
pub fn opted_in(env_var: &str) -> bool {
    std::env::var(env_var).is_ok_and(|value| !value.is_empty())
}

/// The tool-availability check for an opted-in leg.
pub fn require(argv: &[&str]) {
    let ok = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .is_ok_and(|out| out.status.success());
    assert!(ok, "{argv:?} must work when its e2e leg is opted into");
}

/// A deterministic step routing success to `next`, failure to
/// `on_failure`.
pub fn det_step(name: &str, run: &str, next: &str, on_failure: Option<&str>) -> StepDef {
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
        next: Some(next.to_owned()),
        on_failure: on_failure.map(str::to_owned),
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// A decision over `over` routing success onward and failure to abort —
/// the result-gate shape of the reference-stack scenarios.
pub fn gate_step(name: &str, over: &str, success: &str) -> StepDef {
    let mut step = det_step(name, "", "done", None);
    step.kind = StepKind::Decision;
    step.run = None;
    step.next = None;
    step.over = Some(over.to_owned());
    step.on = BTreeMap::from([
        ("success".to_owned(), success.to_owned()),
        ("failure".to_owned(), "abort".to_owned()),
        ("blocked".to_owned(), "abort".to_owned()),
        ("no-work".to_owned(), "abort".to_owned()),
    ]);
    step
}

/// The reference-stack pipeline: `change` leaves a diff (so a green run
/// lands a PR), `check` runs the stack's real verify command, and the
/// `verdict` decision routes its outcome — green onward, red to abort.
pub fn check_steps(change: &str, check: &str) -> Vec<StepDef> {
    let mut check_step = det_step("check", check, "verdict", None);
    check_step.on_failure = Some("verdict".to_owned());
    vec![
        det_step("change", change, "check", None),
        check_step,
        gate_step("verdict", "check", "done"),
    ]
}

/// One test's world: temp dirs, a seeded bare remote, a fake forge.
pub struct Rig {
    pub dir: TestDir,
    pub forge: Arc<FakeForge>,
    url: String,
}

impl Rig {
    /// A world whose remote's main carries `files`.
    pub fn new(files: &[(&str, &str)]) -> Self {
        let dir = TestDir::new("stack");
        let url = make_remote(dir.path(), files);
        Self {
            dir,
            forge: Arc::new(FakeForge::new(vec![item()])),
            url,
        }
    }

    pub fn engine(&self) -> Engine {
        Engine::new(self.dir.path().join("runs"), self.dir.path().join("cache"))
    }

    /// A plan running `steps` against this rig's repo and forge.
    pub fn plan(&self, steps: Vec<StepDef>) -> RunPlan {
        RunPlan {
            run_id: new_run_id("stack-verify").expect("run id"),
            assignment: assignment(),
            pipeline: Pipeline {
                name: "stack-verify".to_owned(),
                steps,
            },
            roles: BTreeMap::new(),
            repos: BTreeMap::from([("main".to_owned(), repo(&self.url))]),
            item: item(),
            forge: self.forge.clone(),
            credentials: BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))]),
            identities: BTreeMap::new(),
            identity_forges: BTreeMap::new(),
            config_source: None,
            plugin_sources: BTreeMap::new(),
            direct_agents: BTreeMap::new(),
            lease: None,
        }
    }

    /// One run's events, read from its log.
    pub fn events(&self, run_id: &str) -> Vec<Event> {
        let dir = self.dir.path().join("runs").join(run_id);
        runlog::read_events(&dir).expect("events read")
    }
}

/// The check step's finished outcome, when it finished.
pub fn check_outcome(events: &[Event]) -> Option<String> {
    events
        .iter()
        .find(|e| {
            e.kind == EventKind::StepFinished
                && e.data.get("step").and_then(serde_json::Value::as_str) == Some("check")
        })
        .and_then(|e| e.data.get("outcome"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
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
        name: "stack-verify".to_owned(),
        work: WorkSource {
            forge: ForgeKind::Github,
            source: "fake".to_owned(),
            filter: "*".to_owned(),
            approval_label: None,
            abort_label: "bureau:failed".to_owned(),
            escalate_label: "bureau:needs-human".to_owned(),
        },
        repos: vec!["main".to_owned()],
        pipeline: "stack-verify".to_owned(),
        role: "worker".to_owned(),
        verify: "true".to_owned(),
        branch_prefix: "bureau/stack/".to_owned(),
        limits: limits(),
    }
}

fn item() -> Item {
    Item {
        external_id: "7".to_owned(),
        title: "Verify the stack".to_owned(),
        body: "Run the real toolchain.".to_owned(),
        url: "fake://item/7".to_owned(),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}
