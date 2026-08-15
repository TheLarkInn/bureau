//! The reference pipeline "fix a failing test" (DESIGN.md section 11)
//! run end to end under the `fake` adapter and forge: offline,
//! deterministic, and fast enough for CI (section 13's acceptance criteria).

mod e2e;

use bureau::contract::StepOutcome;
use bureau::forge::Pr;
use e2e::checks;
use e2e::rig::Rig;
use e2e::steps::{fixture, reference_steps};

/// Writes the propose and review fixtures with the given outcomes.
fn fixtures(rig: &Rig, propose: StepOutcome, review: StepOutcome) -> (String, String) {
    let propose = fixture(rig.dir.path(), "propose.json", propose, "patched");
    let review = fixture(rig.dir.path(), "review.json", review, "looks good");
    (propose, review)
}

/// Runs the pipeline to `Success` and returns its PR.
async fn run_fixed(rig: &Rig, propose: &str, review: &str) -> Pr {
    let plan = rig.plan(reference_steps(propose, review, 3));
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::Success);
    outcome.pr.expect("a PR was opened")
}

/// The central acceptance test: the failing check is reproduced, the
/// agent's fix is applied and verified, and the branch is pushed with a
/// PR — all offline, all replayed from the event log.
#[tokio::test]
async fn the_failing_test_gets_fixed_and_a_pr_lands() {
    let rig = Rig::new();
    let (propose, review) = fixtures(&rig, StepOutcome::Success, StepOutcome::Success);
    let plan = rig.plan(reference_steps(&propose, &review, 3));
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::Success);
    let pr = checks::single_pr(&outcome);
    checks::remote_branch(&rig, &pr.branch);
    checks::replay(&rig, &outcome.run_id, StepOutcome::Success, 6);
    checks::events_in_order(&rig, &outcome.run_id);
}

/// Once the fix lands on the default branch, a fresh run over the same
/// world finds the check already passing and produces no work — and no
/// second PR (the mirror is re-fetched, so it sees the merge).
#[tokio::test]
async fn a_second_run_over_the_merged_fix_is_no_work() {
    let rig = Rig::new();
    let (propose, review) = fixtures(&rig, StepOutcome::Success, StepOutcome::Success);
    let pr = run_fixed(&rig, &propose, &review).await;
    rig.merge_into_main(&pr.branch);
    let plan = rig.plan(reference_steps(&propose, &review, 3));
    let second = rig.engine().run(&plan).await;
    assert_eq!(
        (second.outcome, second.pr.is_none()),
        (StepOutcome::NoWork, true),
        "no changes means NoWork and no PR"
    );
    checks::replay(&rig, &second.run_id, StepOutcome::NoWork, 2);
    checks::pr_count(&rig, 1).await;
}

/// When the agent can never produce a fix, the run escalates: `Blocked`,
/// one comment on the item, and nothing pushed.
#[tokio::test]
async fn a_propose_that_never_succeeds_escalates() {
    let rig = Rig::new();
    let (propose, review) = fixtures(&rig, StepOutcome::Failure, StepOutcome::Success);
    let plan = rig.plan(reference_steps(&propose, &review, 2));
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(outcome.outcome, StepOutcome::Blocked);
    checks::escalated(&rig, "propose");
    checks::pr_count(&rig, 0).await;
}
