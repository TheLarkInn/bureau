use super::{Rig, agent_step, det_step, fixture};
use bureau::contract::StepOutcome;

const fn same_cost(got: f64, want: f64) -> bool {
    got.to_bits() == want.to_bits()
}

#[tokio::test]
async fn measured_usage_sums_into_the_run_total() {
    let rig = Rig::new();
    let transcript = fixture(rig.dir.path(), "bill.json", Some(0.42));
    let steps = vec![
        det_step("edit", "echo changed >> file.txt", "bill"),
        agent_step("bill", &transcript),
    ];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    let pass = outcome.outcome == StepOutcome::Success && same_cost(outcome.cost_usd, 0.42);
    assert!(pass, "outcome: {outcome:?}");
}

#[tokio::test]
async fn adapter_measurement_is_not_agent_clamped() {
    let rig = Rig::new();
    let transcript = fixture(rig.dir.path(), "bill.json", Some(1000.0));
    let steps = vec![agent_step("bill", &transcript)];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    assert!(same_cost(outcome.cost_usd, 1_000.0), "{outcome:?}");
}

#[tokio::test]
async fn measured_usage_sums_across_steps() {
    let rig = Rig::new();
    let big = fixture(rig.dir.path(), "big.json", Some(1000.0));
    let also_big = fixture(rig.dir.path(), "also-big.json", Some(1000.0));
    let mut first = agent_step("bill-one", &big);
    first.next = Some("bill-two".to_owned());
    let second = agent_step("bill-two", &also_big);
    let outcome = rig.engine().run(&rig.plan(vec![first, second])).await;
    assert!(same_cost(outcome.cost_usd, 2_000.0), "{outcome:?}");
}

#[tokio::test]
async fn a_deterministic_step_has_zero_cost() {
    let rig = Rig::new();
    let steps = vec![det_step("edit", "echo changed >> file.txt", "done")];
    let outcome = rig.engine().run(&rig.plan(steps)).await;
    assert!(
        outcome.outcome == StepOutcome::Success && same_cost(outcome.cost_usd, 0.0),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn configured_cost_cap_fails_when_usage_is_unknown() {
    let rig = Rig::new();
    let transcript = fixture(rig.dir.path(), "unknown.json", None);
    let outcome = rig
        .engine()
        .run(&rig.plan(vec![agent_step("bill", &transcript)]))
        .await;
    assert_eq!(outcome.outcome, StepOutcome::Failure, "{outcome:?}");
}

#[tokio::test]
async fn uncapped_assignment_allows_unknown_usage() {
    let rig = Rig::new();
    let transcript = fixture(rig.dir.path(), "unknown.json", None);
    let mut plan = rig.plan(vec![agent_step("bill", &transcript)]);
    plan.assignment.limits.max_cost_per_day_usd = None;
    let outcome = rig.engine().run(&plan).await;
    assert_ne!(outcome.outcome, StepOutcome::Failure, "{outcome:?}");
}
