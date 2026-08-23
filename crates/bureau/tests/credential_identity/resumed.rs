//! Resuming an unfinished run re-checks its credentials.
//!
//! A paused or interrupted run comes back with freshly resolved
//! secrets, and the value behind a reference can have been rotated
//! meanwhile. The resumed run therefore re-asks the forge and compares
//! the answer with the identity its own `run_started` pinned: a changed
//! account aborts before the worktree is re-cut, so no step ever runs
//! as somebody else. A finished run stays idempotent and appends
//! nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bureau::config::StepDef;
use bureau::contract::StepOutcome;
use bureau::engine::RunPlan;

use super::{DECLARED, rig, started_steps};

/// Two steps: the first pauses the run at the boundary before the
/// second, exactly as an interrupted run leaves its log. It also
/// changes a file, so a resumed run that finishes reaches `Success`.
fn paused_steps() -> Vec<StepDef> {
    vec![
        rig::det_step(
            "first",
            "touch ../PAUSE; echo changed >> file.txt",
            Some("second"),
        ),
        rig::det_step("second", "true", Some("done")),
    ]
}

fn pause_path(rig: &rig::Rig, run_id: &str) -> PathBuf {
    rig.dir.path().join("runs").join(run_id).join("PAUSE")
}

/// A plan that pauses after its first step, declaring the identity its
/// credential must have.
fn paused_plan(rig: &rig::Rig) -> RunPlan {
    let mut plan = rig.plan(paused_steps());
    plan.identities = BTreeMap::from([("git-main".to_owned(), DECLARED.to_owned())]);
    plan
}

/// Pauses a run after its first step, with the forge answering as the
/// declared account.
async fn pause_a_run(rig: &rig::Rig) -> RunPlan {
    rig.forge.verify_identity_as(DECLARED);
    let plan = paused_plan(rig);
    let paused = rig.engine().run(&plan).await;
    assert!(paused.message.contains("paused"), "{}", paused.message);
    std::fs::remove_file(pause_path(rig, &plan.run_id)).expect("PAUSE removes");
    plan
}

/// The credential now authenticates as another account: the resumed run
/// aborts before its next step, naming the reference and the identity
/// the run started with.
#[tokio::test]
async fn a_changed_account_aborts_the_resumed_run_before_its_next_step() {
    let rig = rig::Rig::new();
    let plan = pause_a_run(&rig).await;
    rig.forge.verify_identity_as("someone-else");
    let resumed = rig.engine().run(&plan).await;
    let message = resumed.message;
    assert_eq!(
        (
            resumed.outcome,
            started_steps(&rig, &plan.run_id),
            message.contains("git-main") && message.contains("this run started with"),
            message.contains("someone-else") && !message.contains("test-credential"),
        ),
        (StepOutcome::Failure, 1, true, true)
    );
}

/// The identity is pinned, not re-read from settings: a resumed run
/// whose declaration was edited meanwhile still has to match what its
/// own `run_started` recorded.
#[tokio::test]
async fn a_resumed_run_matches_the_pinned_identity_not_the_current_declaration() {
    let rig = rig::Rig::new();
    let mut plan = pause_a_run(&rig).await;
    rig.forge.verify_identity_as("someone-else");
    plan.identities = BTreeMap::from([("git-main".to_owned(), "someone-else".to_owned())]);
    let resumed = rig.engine().run(&plan).await;
    assert_eq!(
        (
            resumed.outcome,
            super::pinned(&rig, &plan.run_id).get("git-main").cloned(),
        ),
        (StepOutcome::Failure, Some(DECLARED.to_owned()))
    );
}

/// A credential the forge still accepts as the pinned account resumes
/// normally, finishing the run it left unfinished.
#[tokio::test]
async fn an_unchanged_account_resumes_the_run() {
    let rig = rig::Rig::new();
    let plan = pause_a_run(&rig).await;
    let resumed = rig.engine().run(&plan).await;
    assert_eq!(
        (resumed.outcome, started_steps(&rig, &plan.run_id)),
        (StepOutcome::Success, 2)
    );
}

/// A finished run is idempotent: re-entry returns the recorded outcome
/// without verifying anything again or appending an event.
#[tokio::test]
async fn a_finished_run_appends_nothing_and_verifies_nothing() {
    let rig = rig::Rig::new();
    let plan = pause_a_run(&rig).await;
    let finished = rig.engine().run(&plan).await;
    let before = super::events(&rig, &plan.run_id).len();
    rig.forge.verify_identity_as("someone-else");
    let again = rig.engine().run(&plan).await;
    assert_eq!(
        (
            finished.outcome,
            again.outcome,
            super::events(&rig, &plan.run_id).len(),
        ),
        (StepOutcome::Success, StepOutcome::Success, before)
    );
}
