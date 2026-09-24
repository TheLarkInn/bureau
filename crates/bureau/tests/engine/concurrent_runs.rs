//! Concurrent runs of one repo share its mirror. Their refreshes and
//! branch cuts hold the mirror lock, so no run fails on another's ref
//! updates or loses its unpushed branch to another's prune.

use bureau::contract::StepOutcome;
use tokio::task::JoinSet;

use super::rig::{Rig, det_step};

/// Concurrent runs per round.
const RUNS: usize = 4;
/// Rounds; the first one clones the mirror, the rest fetch.
const ROUNDS: usize = 3;

/// Commits to `main` in the rig repo, which is the runs' remote.
fn advance(rig: &Rig, round: usize) {
    let message = format!("round {round}");
    let status = std::process::Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@test"])
        .args(["commit", "--allow-empty", "-m", &message])
        .current_dir(&rig.url)
        .status()
        .expect("git runs");
    assert!(status.success(), "advancing the remote failed");
}

/// Starts `RUNS` runs at once; returns each outcome and its message.
async fn run_all(rig: &Rig, round: usize) -> Vec<(StepOutcome, String)> {
    let mut runs = JoinSet::new();
    for run in 0..RUNS {
        let command = format!("echo {round}-{run} >> file.txt");
        let plan = rig.plan(vec![det_step("edit", &command, Some("done"))]);
        let engine = rig.engine();
        runs.spawn(async move { engine.run(&plan).await });
    }
    let outcomes = runs.join_all().await.into_iter();
    outcomes.map(|done| (done.outcome, done.message)).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_runs_of_one_repo_all_succeed() {
    let rig = Rig::new();
    let mut failed = Vec::new();
    for round in 0..ROUNDS {
        advance(&rig, round);
        let outcomes = run_all(&rig, round).await.into_iter();
        failed.extend(outcomes.filter(|(outcome, _)| *outcome != StepOutcome::Success));
    }
    assert_eq!(failed, Vec::new());
}
