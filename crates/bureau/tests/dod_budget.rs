//! Definition-of-done proofs (DESIGN.md section 13): an exhausted
//! hourly budget blocks a run before any spawn, no secret lands anywhere
//! under `runs/`, an unchanged item never re-runs, and an unresolvable
//! credential fails `bureau run` before any run directory exists.

#[path = "dodb/fixtures.rs"]
mod fixtures;
#[path = "dodb/world.rs"]
mod world;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use bureau::config::Limits;
use bureau::contract::StepOutcome;
use bureau::engine::RunOutcome;
use bureau::process::{REDACTED, Secret};
use bureau::reconcile::Started;
use bureau::runlog::{self, Event, EventKind};
use fixtures::{ASSIGNMENT, TestDir, generous};
use world::{EngineRig, World};

/// The credential the scrub test has a step print, as a leaked token
/// would appear; the spawn's scrub list must redact it everywhere.
const TOKEN: &str = "test-token";

/// §13: exceeding an hourly limit blocks the run before any subprocess spawns.
#[tokio::test]
async fn an_exhausted_hourly_limit_spawns_nothing() {
    let limits = Limits {
        max_runs_per_hour: Some(2),
        ..generous()
    };
    let world = World::new(&["1", "2"], "echo changed >> file.txt", limits);
    for _ in 0..2 {
        world.store.record_run(ASSIGNMENT, 0.0).expect("record_run");
    }
    let started = world.pass().await;
    let gate = (started.len(), world.run_dirs(), world.leased().len());
    assert_eq!(gate, (0, 0, 0), "nothing claimed, spawned, or written");
}

/// §13: no secret appears anywhere under `runs/`. Deterministic steps
/// receive no credentials in their environment (section 10), so the
/// step prints the token literally — the spawn's scrub list still
/// holds every resolved plan credential and must redact it.
#[tokio::test]
async fn a_step_credential_never_lands_under_runs() {
    let rig = EngineRig::new();
    let credentials = BTreeMap::from([
        ("git-main".to_owned(), Secret::new("test-credential")),
        (TOKEN.to_owned(), Secret::new(TOKEN)),
    ]);
    let outcome = rig.run(&format!("echo {TOKEN}"), credentials).await;
    assert_eq!(outcome.outcome, StepOutcome::NoWork);
    check_tree_scrubbed(&rig.runs().join(&outcome.run_id));
    check_output_events(&run_events(&rig, &outcome.run_id));
}

/// One run's events, read back from its log on disk.
fn run_events(rig: &EngineRig, run_id: &str) -> Vec<Event> {
    runlog::read_events(&rig.runs().join(run_id)).expect("events read")
}

/// Every file under `dir` as (path, lossy text), recursively.
fn tree_texts(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    collect_texts(dir, &mut files);
    files
}

/// Appends `dir`'s files to `files`, descending into subdirectories.
fn collect_texts(dir: &Path, files: &mut Vec<(PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).expect("walk reads") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect_texts(&path, files);
        } else {
            let bytes = std::fs::read(&path).expect("file reads");
            files.push((path, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
}

/// Every file under the run dir is secret-free; the redaction marker
/// proves the scrub actually ran.
fn check_tree_scrubbed(run_dir: &Path) {
    let files = tree_texts(run_dir);
    let leaks: Vec<&Path> = files
        .iter()
        .filter(|(_, text)| text.contains(TOKEN))
        .map(|(path, _)| path.as_path())
        .collect();
    let redacted = files
        .iter()
        .filter(|(_, text)| text.contains(REDACTED))
        .count();
    let names: Vec<&Path> = files.iter().map(|(path, _)| path.as_path()).collect();
    let verdict = (files.len() >= 2, leaks.is_empty(), redacted > 0);
    assert_eq!(
        verdict,
        (true, true, true),
        "files: {names:?}, leaks: {leaks:?}"
    );
}

/// The step's captured stdout in the output events is the scrubbed form.
fn check_output_events(events: &[Event]) {
    let chunks: Vec<&str> = events
        .iter()
        .filter(|event| event.kind == EventKind::Output)
        .filter_map(|event| event.data.get("data").and_then(serde_json::Value::as_str))
        .collect();
    let leaked = chunks.iter().filter(|data| data.contains(TOKEN)).count();
    let scrubbed = chunks.iter().filter(|data| data.contains(REDACTED)).count();
    let seen = (!chunks.is_empty(), leaked == 0, scrubbed >= 1);
    assert_eq!(seen, (true, true, true), "captured stdout: {chunks:?}");
}

/// §13: re-reconciling an unchanged item produces no new run — even
/// when the first run found no work and opened no PR. The spawn
/// wrapper marks the item's content seen on every terminal outcome but
/// `Failure` (src/reconcile/dedup.rs), so the second pass finds the
/// hash in the dedup table and starts nothing.
#[tokio::test]
async fn an_unchanged_item_is_not_reconciled_twice() {
    let world = World::new(&["1"], "true", generous());
    let outcomes = join_all(world.pass().await).await;
    let first = (
        outcomes.len(),
        outcomes[0].outcome,
        outcomes[0].pr.is_some(),
    );
    assert_eq!(first, (1, StepOutcome::NoWork, false));
    let state = second_pass(&world).await;
    assert_eq!(state, (0, 0, 0), "no re-run, no lease, no PR");
}

/// Pass 2's world state: started runs, live leases, observed PRs.
async fn second_pass(world: &World) -> (usize, usize, usize) {
    let started = world.pass().await;
    (
        started.len(),
        world.leased().len(),
        world.observed_prs().await,
    )
}

/// Joins every started run, collecting outcomes.
async fn join_all(started: Vec<Started>) -> Vec<RunOutcome> {
    let mut outcomes = Vec::new();
    for run in started {
        outcomes.push(run.handle.await.expect("run joins"));
    }
    outcomes
}

/// §13: a step missing a required credential fails before spawn, naming it.
#[test]
fn a_missing_credential_fails_before_any_run_dir_exists() {
    let dir = TestDir::new("cli");
    write_minimal_config(dir.path());
    let output = bureau_run(dir.path());
    let err = stderr(&output);
    let runs = dir.path().join("runs");
    let spawned = runs.is_dir() && std::fs::read_dir(&runs).expect("reads").next().is_some();
    let verdict = (
        output.status.code(),
        err.contains("gh-main"),
        spawned,
        dir.path().join("state.db").exists(),
    );
    assert_eq!(verdict, (Some(2), true, false, false), "{err}");
}

/// `bureau run` against the config in `dir`, every root inside `dir`,
/// with the credential environment unset.
fn bureau_run(dir: &Path) -> Output {
    let args = run_args(dir);
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(&args)
        .current_dir(dir)
        .env_remove("BUREAU_CREDENTIAL_GH_MAIN")
        .env_remove("BUREAU_CREDENTIALS_DIR")
        .output()
        .expect("run bureau")
}

/// The run verb's argv, with every filesystem root inside `dir`.
fn run_args(dir: &Path) -> Vec<String> {
    let lead = ["run", "fix-failing-test", "--item", "42", "--config"];
    let mut args: Vec<String> = lead.into_iter().map(str::to_owned).collect();
    args.push(dir.to_string_lossy().into_owned());
    for (flag, name) in [
        ("--runs", "runs"),
        ("--state", "state.db"),
        ("--cache", "cache"),
    ] {
        args.push(flag.to_owned());
        args.push(dir.join(name).to_string_lossy().into_owned());
    }
    args
}

/// The process's stderr as text.
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const MINIMAL_REPO: &str = r"
repos:
  code:
    url: https://github.com/example/code
    forge: github
    access: push
    credential: gh-main
";

const MINIMAL_ROLE: &str = r"
name: worker
agent: agents/worker.md
adapter: fake
permissions: [repo:read, repo:write, pr:write]
min_trust: untrusted
";

const MINIMAL_PIPELINE: &str = r#"
name: fix-failing-test
steps:
  - name: work
    type: deterministic
    run: "true"
    next: done
"#;

const MINIMAL_ASSIGNMENT: &str = r#"
name: job
work:
  forge: github
  source: "example/code"
  filter: "label:agent-eligible"
repos: [code]
pipeline: fix-failing-test
role: worker
verify: "make test"
branch_prefix: runner/
limits:
  max_concurrent: 1
  max_runs_per_hour: 4
  max_runs_per_day: 20
  max_open_prs: 3
  max_cost_per_day_usd: 10
"#;

/// A config whose one repo credential nothing in the env can resolve.
fn write_minimal_config(dir: &Path) {
    write(dir, "repos.yaml", MINIMAL_REPO);
    write(dir, "roles/worker.yaml", MINIMAL_ROLE);
    write(dir, "assignments/job.yaml", MINIMAL_ASSIGNMENT);
    write(dir, "pipelines/fix-failing-test.yaml", MINIMAL_PIPELINE);
}

/// Writes `text` to `name` under `dir`, creating parents.
fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
}
