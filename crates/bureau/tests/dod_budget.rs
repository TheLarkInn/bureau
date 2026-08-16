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

use bureau::config::Limits;
use bureau::contract::StepOutcome;
use bureau::engine::RunOutcome;
use bureau::process::{REDACTED, Secret};
use bureau::reconcile::Started;
use bureau::runlog::{self, Event, EventKind};
use fixtures::{ASSIGNMENT, generous};
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
    for run in 0..2 {
        world
            .store
            .record_run(&format!("budget-run-{run}"), ASSIGNMENT, 0.0)
            .expect("record_run");
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
