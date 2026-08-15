//! Engine cost-accounting tests (DESIGN.md section 6): a step's claimed
//! `cost_usd` is clamped into `0.0..=25.0` before it sums into the run total.

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

fn test_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A temporary directory removed on drop.
struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-cost-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
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

/// A step result claiming `cost` USD.
fn claim(cost: f64) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Success,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        cost_usd: cost,
        message: "done".to_owned(),
    }
}

/// Writes a `fake` adapter fixture whose stdout is the step result.
fn fixture(dir: &Path, name: &str, cost: f64) -> String {
    let json = claim(cost).to_json().expect("result serializes");
    let mut data = String::from_utf8(json).expect("utf8");
    data.push('\n');
    let transcript = Transcript {
        schema: SCHEMA_VERSION.to_owned(),
        chunks: vec![Chunk {
            delay_ms: 0,
            stream: Stream::Stdout,
            data,
        }],
        exit_code: 0,
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
        next: None,
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
    }
}

/// A deterministic step running `run`, routing to `next`.
fn det_step(name: &str, run: &str, next: &str) -> StepDef {
    let mut step = step(name, StepKind::Deterministic);
    step.run = Some(run.to_owned());
    step.next = Some(next.to_owned());
    step
}

/// An agent step replaying `fixture`, routing to `done`.
fn agent_step(name: &str, fixture: &str) -> StepDef {
    let mut step = step(name, StepKind::Agent);
    step.role = Some("worker".to_owned());
    step.fixture = Some(fixture.to_owned());
    step.next = Some("done".to_owned());
    step
}

/// The worker role, replayed by the `fake` adapter.
fn role() -> Role {
    Role {
        name: "worker".to_owned(),
        agent: "/fake:worker".to_owned(),
        adapter: AdapterKind::Fake,
        model: "fake".to_owned(),
        permissions: Vec::new(),
        min_trust: Trust::Untrusted,
        concurrency: 1,
    }
}

/// The registry entry for the local repo.
fn repo(url: &str) -> Repo {
    Repo {
        url: url.to_owned(),
        forge: ForgeKind::Github,
        access: Access::Push,
        credential: "git-main".to_owned(),
    }
}

/// The work item the fake forge hands out.
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

/// The assignment every test runs, with section 6 limits present.
fn assignment() -> Assignment {
    Assignment {
        name: "fix-tests".to_owned(),
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
        limits: Limits {
            max_concurrent: 1,
            max_runs_per_hour: 10,
            max_runs_per_day: 20,
            max_open_prs: 5,
            max_cost_per_day_usd: 50.0,
        },
    }
}

/// One test's world: temp dirs, a local repo, one item on a fake forge.
struct Rig {
    dir: TestDir,
    url: String,
}

impl Rig {
    fn new() -> Self {
        let dir = TestDir::new();
        let url = make_repo(dir.path());
        Self { dir, url }
    }

    fn engine(&self) -> Engine {
        let (runs, cache) = (self.dir.path().join("runs"), self.dir.path().join("cache"));
        Engine::new(runs, cache, test_clock)
    }

    /// A plan for `steps`; the repo credential resolves so pushes work.
    fn plan(&self, steps: Vec<StepDef>) -> RunPlan {
        RunPlan {
            run_id: new_run_id("fix-tests", test_clock()),
            assignment: assignment(),
            pipeline: Pipeline {
                name: "fix".to_owned(),
                steps,
            },
            roles: BTreeMap::from([("worker".to_owned(), role())]),
            repos: BTreeMap::from([("main".to_owned(), repo(&self.url))]),
            item: item(),
            forge: Arc::new(FakeForge::new(vec![item()])),
            credentials: BTreeMap::from([("git-main".to_owned(), Secret::new("test-credential"))]),
            daemon_env: BTreeMap::new(),
        }
    }
}

/// Bit-exact cost equality: the clamp only pins to exact bounds.
const fn same_cost(got: f64, want: f64) -> bool {
    got.to_bits() == want.to_bits()
}

#[tokio::test]
async fn an_honest_claim_sums_into_the_run_total() {
    let rig = Rig::new();
    let transcript = fixture(rig.dir.path(), "bill.json", 0.42);
    let steps = vec![
        det_step("edit", "echo changed >> file.txt", "bill"),
        agent_step("bill", &transcript),
    ];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    let pass = outcome.outcome == StepOutcome::Success && same_cost(outcome.cost_usd, 0.42);
    assert!(pass, "outcome: {outcome:?}");
}

#[tokio::test]
async fn an_inflated_claim_clamps_to_the_cap() {
    let rig = Rig::new();
    let transcript = fixture(rig.dir.path(), "bill.json", 1000.0);
    let steps = vec![agent_step("bill", &transcript)];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    let pass = same_cost(outcome.cost_usd, 25.0);
    assert!(pass, "outcome: {outcome:?}");
}

#[tokio::test]
async fn the_clamp_applies_per_step_not_per_run() {
    let rig = Rig::new();
    let big = fixture(rig.dir.path(), "big.json", 1000.0);
    let also_big = fixture(rig.dir.path(), "also-big.json", 1000.0);
    let mut first = agent_step("bill-one", &big);
    first.next = Some("bill-two".to_owned());
    let second = agent_step("bill-two", &also_big);
    let outcome = rig.engine().run(&rig.plan(vec![first, second])).await;
    let pass = same_cost(outcome.cost_usd, 50.0);
    assert!(pass, "outcome: {outcome:?}");
}

#[tokio::test]
async fn a_deterministic_step_claims_no_cost() {
    let rig = Rig::new();
    let steps = vec![det_step("edit", "echo changed >> file.txt", "done")];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    let pass = outcome.outcome == StepOutcome::Success && same_cost(outcome.cost_usd, 0.0);
    assert!(pass, "outcome: {outcome:?}");
}
