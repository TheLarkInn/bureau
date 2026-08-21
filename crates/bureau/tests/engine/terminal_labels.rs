use bureau::contract::StepOutcome;
use bureau::forge::Forge as _;
use bureau::runlog::{self, RunTerminal};

use super::rig::{Rig, det_step};

fn terminal(rig: &Rig, run_id: &str) -> Option<RunTerminal> {
    let directory = rig.dir.path().join("runs").join(run_id);
    runlog::replay_state(&directory)
        .expect("replayed state")
        .finished
        .and_then(|finished| finished.terminal)
}

#[tokio::test]
async fn a_missing_edge_adds_the_abort_label() {
    let rig = Rig::new();
    let outcome = rig
        .engine()
        .run(&rig.plan(vec![det_step("edit", "true", None)]))
        .await;
    assert_eq!(
        (
            outcome.outcome,
            outcome.message.contains("no edge"),
            rig.forge.labels_of("42"),
            terminal(&rig, &outcome.run_id),
        ),
        (
            StepOutcome::Failure,
            true,
            vec!["bureau:failed".to_owned()],
            Some(RunTerminal::Abort),
        )
    );
}

#[tokio::test]
async fn escalation_swaps_the_abort_label_for_attention() {
    let rig = Rig::new();
    rig.forge
        .set_labels("42", &["bug".to_owned(), "bureau:failed".to_owned()])
        .await
        .expect("seed labels");
    let mut step = det_step("verify", "false", Some("done"));
    step.on_failure = Some("escalate".to_owned());
    let outcome = rig.engine().run(&rig.plan(vec![step])).await;
    assert_eq!(
        (
            outcome.outcome,
            rig.forge.labels_of("42"),
            rig.forge.comments().len(),
            terminal(&rig, &outcome.run_id),
        ),
        (
            StepOutcome::Blocked,
            vec!["bug".to_owned(), "bureau:needs-human".to_owned()],
            1,
            Some(RunTerminal::Escalate),
        )
    );
}
