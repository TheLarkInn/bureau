//! Support for the wired-path behavior port: config files on disk
//! loaded through `Config::load`, a reconcile pass driving the engine,
//! and plugin-referenced roles resolving the shipped `bureau` plugin.
//! Offline only (DESIGN.md section 12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::adapters::fake::{Chunk, Stream, Transcript};
use bureau::config::Config;
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};
use bureau::engine::{Engine, RunOutcome};
use bureau::forge::fake::FakeForge;
use bureau::forge::{Forge, Item};
use bureau::process::Secret;
use bureau::reconcile::Reconciler;
use bureau::runlog::ConfigSource;
use bureau::state::Store;

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

/// A bare remote holding one committed file, so the run needs no
/// network access.
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

/// The wired config's fixed commit identity, as a fetched committed
/// source would carry.
pub fn config_source() -> ConfigSource {
    ConfigSource {
        remote: "fixture".to_owned(),
        reference: "main".to_owned(),
        commit: "0000000000000000000000000000000000000000".to_owned(),
    }
}

/// The wired pipeline: implement and review are agent steps whose roles
/// reference the shipped plugin's agents; the decision gates local-ci.
const PIPELINE: &str = r"
name: fix-failing-test
steps:
  - name: implement
    type: agent
    role: implementer
    fixture: {IMPLEMENT}
    next: apply
    on_failure: escalate
    on_blocked: escalate
  - name: apply
    type: deterministic
    run: echo change >> impl.txt
    next: review
  - name: review
    type: agent
    role: reviewer
    fixture: {REVIEW}
    next: verdict
    on_failure: escalate
  - name: verdict
    type: decision
    over: review
    on: {success: local-ci, failure: implement, blocked: escalate, no-work: abort}
  - name: local-ci
    type: deterministic
    run: test -f impl.txt
    next: done
";

/// Writes the config tree (repos, roles, assignment, pipeline) for the
/// skeleton, with the fixture paths substituted in.
fn write_config(dir: &Path, url: &str, implement: &str, review: &str) {
    let repos = format!(
        "repos:\n  main:\n    url: {url}\n    forge: github\n    access: push\n    credential: git-main\n"
    );
    let implementer = "name: implementer\nagent: /bureau:implementer\nadapter: fake\npermissions: [repo:read, repo:write, repo:push, pr:write]\nmin_trust: untrusted\n";
    let reviewer = "name: reviewer\nagent: /bureau:reviewer\nadapter: fake\npermissions: [repo:read, pr:write]\nmin_trust: untrusted\n";
    let assignment = "name: fix-failing-test\nwork:\n  forge: github\n  source: fake\n  filter: '*'\nrepos: [main]\npipeline: fix-failing-test\nrole: implementer\nverify: test -f impl.txt\nbranch_prefix: bureau/fix/\n".to_owned();
    let pipeline = PIPELINE
        .replace("{IMPLEMENT}", implement)
        .replace("{REVIEW}", review);
    let files = [
        ("repos.yaml", repos),
        ("roles/implementer.yaml", implementer.to_owned()),
        ("roles/reviewer.yaml", reviewer.to_owned()),
        ("assignments/fix-failing-test.yaml", assignment),
        ("pipelines/fix-failing-test.yaml", pipeline),
    ];
    for (name, text) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
        std::fs::write(path, text).expect("write config");
    }
}

/// One test's world: config on disk, loaded and validated, driving a
/// reconciler over one item on the fake forge.
pub struct World {
    pub dir: TestDir,
    pub forge: Arc<FakeForge>,
    pub engine: Arc<Engine>,
    pub reconciler: Reconciler,
}

impl World {
    /// A world whose config tree loads clean and claims one item.
    pub fn new() -> Self {
        let dir = TestDir::new("wired");
        let url = make_remote(dir.path());
        let implement = fixture(
            dir.path(),
            "implement.json",
            StepOutcome::Success,
            "implemented",
        );
        let review = fixture(
            dir.path(),
            "review.json",
            StepOutcome::Success,
            "looks good",
        );
        let config_dir = dir.path().join("config");
        write_config(&config_dir, &url, &implement, &review);
        let config = Config::load(&config_dir).expect("config loads");
        Self::from_config(dir, config)
    }

    /// Assembles the reconciler over the loaded config.
    fn from_config(dir: TestDir, config: Config) -> Self {
        let forge = Arc::new(FakeForge::new(vec![item()]));
        let store = Arc::new(Store::open_in_memory().expect("in-memory store"));
        let engine = Arc::new(Engine::new(
            dir.path().join("runs"),
            dir.path().join("cache"),
        ));
        let reconciler = Reconciler {
            config,
            state: store,
            forges: BTreeMap::from([(
                "fix-failing-test".to_owned(),
                forge.clone() as Arc<dyn Forge>,
            )]),
            engine: engine.clone(),
            credentials: BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))]),
            config_source: config_source(),
            direct_agents: BTreeMap::new(),
        };
        Self {
            dir,
            forge,
            engine,
            reconciler,
        }
    }

    /// One pass plus every started run's joined outcome.
    pub async fn pass_and_join(&self) -> Vec<RunOutcome> {
        let started = self.reconciler.reconcile_once().await.expect("pass");
        let mut outcomes = Vec::new();
        for run in started {
            outcomes.push(run.handle.await.expect("run joins"));
        }
        outcomes
    }

    /// The primary repo's registry URL, as the observation resolves it.
    pub fn primary_url(&self) -> String {
        self.reconciler
            .config
            .repos
            .get("main")
            .expect("main repo")
            .url
            .clone()
    }
}

/// The claimed work item the run operates on.
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
