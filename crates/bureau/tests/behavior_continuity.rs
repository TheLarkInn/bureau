//! The run-branch continuity behavior port (goober's test/e2e/
//! `run_branch_continuity_test.go`): a later step's worktree carries the
//! implement stage's committed change, and two concurrent runs over one
//! engine never collide on a branch.

#[path = "behavior/continuity_support.rs"]
mod support;

use bureau::contract::StepOutcome;
use bureau::forge::Forge as _;
use bureau::runlog::{self, EventKind};
use support::Rig;

/// The scenario's two agent fixtures, both passing.
fn fixtures(rig: &Rig) -> (String, String) {
    (
        support::fixture(
            rig.dir.path(),
            "implement.json",
            StepOutcome::Success,
            "implemented",
        ),
        support::fixture(
            rig.dir.path(),
            "review.json",
            StepOutcome::Success,
            "looks good",
        ),
    )
}

/// The continuity regression port (goober #133): local-ci runs against
/// the implement stage's committed change, not a pristine base — its
/// `test -f IMPLEMENTED` passes only when the run's worktree carries
/// the earlier step's commit.
#[tokio::test]
async fn local_ci_sees_the_implement_steps_commit() {
    let rig = Rig::new();
    let url = rig.add_remote("remote");
    let (implement, review) = fixtures(&rig);
    let steps = support::continuity_steps(&implement, &review);
    let outcome = rig.engine().run(&rig.plan(&url, "101", steps)).await;
    let events = rig.dir.path().join("runs").join(&outcome.run_id);
    let events = runlog::read_events(&events).expect("events read");
    let local_ci = events.iter().find(|e| {
        e.kind == EventKind::StepFinished
            && e.data.get("step").and_then(serde_json::Value::as_str) == Some("local-ci")
    });
    let seen = (
        outcome.outcome,
        local_ci
            .and_then(|e| e.data.get("outcome"))
            .and_then(serde_json::Value::as_str),
        outcome.pr.is_some(),
    );
    assert_eq!(seen, (StepOutcome::Success, Some("success"), true));
}

/// The concurrency port: two runs with distinct run ids over one engine
/// (one shared checkout cache) both complete, and their branches —
/// keyed on the run id — differ. Two remotes keep the shared mirror
/// out of the assertion; the branch namespace is what is under test.
#[tokio::test]
async fn two_concurrent_runs_get_distinct_branches() {
    let rig = Rig::with_items(vec![item("1"), item("2")]);
    let (implement, review) = fixtures(&rig);
    let steps = support::continuity_steps(&implement, &review);
    let engine = rig.engine();
    let first = rig.plan(&rig.add_remote("remote-a"), "1", steps.clone());
    let second = rig.plan(&rig.add_remote("remote-b"), "2", steps);
    let (one, two) = tokio::join!(engine.run(&first), engine.run(&second));
    check_distinct(&rig, &one, &two).await;
}

/// Both runs completed, their branches differ, and the forge holds both.
async fn check_distinct(
    rig: &Rig,
    one: &bureau::engine::RunOutcome,
    two: &bureau::engine::RunOutcome,
) {
    let branches = (
        one.pr.as_ref().map(|pr| pr.branch.clone()),
        two.pr.as_ref().map(|pr| pr.branch.clone()),
    );
    let prs = rig
        .forge
        .open_prs("main", "bureau/fix/")
        .await
        .expect("open_prs")
        .len();
    assert_eq!(
        (one.outcome, two.outcome, branches.0 != branches.1, prs),
        (StepOutcome::Success, StepOutcome::Success, true, 2)
    );
}

/// A work item with id-derived content.
fn item(id: &str) -> bureau::forge::Item {
    bureau::forge::Item {
        external_id: id.to_owned(),
        title: format!("Fix {id}"),
        body: format!("{id} is broken"),
        url: format!("fake://item/{id}"),
        labels: Vec::new(),
        trust: bureau::contract::Trust::Untrusted,
    }
}
