//! Support for the walking-skeleton behavior ports: a local git remote,
//! the fake adapter and forge, pipeline builders for the skeleton shape,
//! and event-log assertions. Offline only (DESIGN.md section 12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::adapters::AdapterKind;
use bureau::adapters::fake::{Chunk, Stream, Transcript};
use bureau::config::{
    Access, Assignment, ForgeKind, Limits, Pipeline, Repo, Role, StepDef, WorkSource,
};
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};
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

/// A bare remote holding one committed file, so the skeleton needs no
/// network access (the goober suite's fixture-repo acceptance).
fn make_remote(parent: &Path) -> String {
    let work = parent.join("work");
    git(parent, &["init", "-b", "main", "work"]);
    std::fs::write(work.join("file.txt"), "start\n").expect("seed file");
    git(&work, &["add", "-A"]);
    let identity = ["-c", "user.name=test", "-c", "user.email=test@test"];
    git(
        &work,
        &[&identity[..], &["commit", "-m", "init"][..]].concat(),
    );
    git(parent, &["clone", "--bare", "work", "remote.git"]);
    parent.join("remote.git").to_string_lossy().into_owned()
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

/// One test's world: temp dirs, a bare remote, one item on a fake forge.
pub struct Rig {
    pub dir: TestDir,
    pub forge: Arc<FakeForge>,
    url: String,
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
        name: "fix-failing-test".to_owned(),
        work: WorkSource {
            forge: ForgeKind::Github,
            source: "fake".to_owned(),
            filter: "*".to_owned(),
            approval_label: None,
        },
        repos: vec!["main".to_owned()],
        pipeline: "skeleton".to_owned(),
        role: "implementer".to_owned(),
        verify: "test -f impl.txt".to_owned(),
        branch_prefix: "bureau/fix/".to_owned(),
        limits: limits(),
    }
}

fn item() -> Item {
    Item {
        external_id: "101".to_owned(),
        title: "Add walking skeleton smoke path".to_owned(),
        body: "Prove the pipeline end to end.".to_owned(),
        url: "fake://item/101".to_owned(),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

impl Rig {
    /// A world whose remote holds one committed file.
    pub fn new() -> Self {
        let dir = TestDir::new("skeleton");
        let url = make_remote(dir.path());
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
            run_id: new_run_id("fix-failing-test").expect("run id"),
            assignment: assignment(),
            pipeline: Pipeline {
                name: "skeleton".to_owned(),
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

    /// One run's events, read from its log.
    pub fn events(&self, run_id: &str) -> Vec<Event> {
        let dir = self.dir.path().join("runs").join(run_id);
        runlog::read_events(&dir).expect("events read")
    }
}

/// Whether the event's `step` payload field is `name`.
fn step_is(event: &Event, name: &str) -> bool {
    event.data.get("step").and_then(serde_json::Value::as_str) == Some(name)
}

/// (`step_started`, `step_finished`) counts for one step.
pub fn step_counts(events: &[Event], step: &str) -> (usize, usize) {
    let count = |kind| {
        events
            .iter()
            .filter(|e| e.kind == kind && step_is(e, step))
            .count()
    };
    (
        count(EventKind::StepStarted),
        count(EventKind::StepFinished),
    )
}

/// Sequence numbers start at 0 and increase by exactly one.
pub fn check_seq(events: &[Event]) {
    assert_eq!(events.first().map(|e| e.seq), Some(0));
    assert!(
        events.windows(2).all(|w| w[1].seq == w[0].seq + 1),
        "sequence numbers are contiguous"
    );
}

/// The conformance-style projection of a log: kind, step, and outcome
/// per event — run ids, timestamps, and branch payloads excluded, the
/// same normalization the goober suite's `ConformanceView` performs.
pub fn normalized(events: &[Event]) -> Vec<(EventKind, Option<String>, Option<String>)> {
    events
        .iter()
        .map(|e| {
            (
                e.kind,
                e.data
                    .get("step")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                e.data
                    .get("outcome")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            )
        })
        .collect()
}
