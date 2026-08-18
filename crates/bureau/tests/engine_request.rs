//! What a step actually receives on its stdin: the run's work item, so a
//! step never has to go looking for its own assignment (issue #22).

#[path = "engine/rig.rs"]
mod rig;

use bureau::contract::StepOutcome;
use rig::{Rig, det_step};

/// The rig is included per test binary, so helpers this file does not
/// need would otherwise be dead code. Same convention as
/// `engine_durability.rs`; `#[allow]` is not available here.
fn use_fixture_helpers(rig: &Rig) {
    let result = rig::result(StepOutcome::Success, "unused");
    let fixture = rig::fixture(rig.dir.path(), "unused.json", &result);
    let _ = (
        rig::step("unused", bureau::config::StepKind::Deterministic),
        rig::agent_step("unused", &fixture, None),
        rig::decision_step("unused", "other"),
    );
}

/// Runs one deterministic step whose command reads the request on stdin.
///
/// `grep` exits 1 when the pattern is absent, which fails the step, so
/// the terminal outcome is the assertion: `NoWork` means the step saw
/// what it was looking for, anything else means it did not.
async fn grep_the_request(pattern: &str) -> StepOutcome {
    let rig = Rig::new();
    use_fixture_helpers(&rig);
    let run = format!("grep -q '{pattern}'");
    let steps = vec![det_step("check", &run, Some("done"))];
    rig.engine().run(&rig.plan(steps)).await.outcome
}

#[tokio::test]
async fn a_step_receives_the_work_item_it_is_acting_on() {
    // Values from the rig's item, not field names: `external_id` would
    // appear in the JSON even when the item is empty, so it proves
    // nothing.
    let carried = ["fake://item/42", "Fix the thing", "It is broken."];
    let mut outcomes = Vec::new();
    for pattern in carried {
        outcomes.push(grep_the_request(pattern).await);
    }
    assert_eq!(outcomes, vec![StepOutcome::NoWork; carried.len()]);
}

#[tokio::test]
async fn the_grep_probe_fails_when_the_text_is_absent() {
    // Guards the probe itself: without this, the test above would pass
    // even if `grep` always succeeded.
    let outcome = grep_the_request("a-string-no-request-contains").await;
    assert_ne!(outcome, StepOutcome::NoWork);
}
