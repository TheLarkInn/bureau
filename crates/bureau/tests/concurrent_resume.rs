//! Crash recovery resumes only unfinished concurrent members.

#[path = "engine/rig.rs"]
mod rig;

use std::path::Path;
use std::time::{Duration, Instant};

use bureau::config::{Completion, StepKind};
use bureau::contract::{StepOutcome, Trust};
use bureau::engine::{Engine, RunPlan};
use bureau::runlog::{self, Event, EventKind};
use tokio::runtime::Builder;

#[tokio::test]
async fn restart_skips_finished_concurrent_member() {
    let rig = rig::Rig::new();
    use_helpers(&rig);
    let mut plan = rig.plan(Vec::new());
    let run_dir = rig.dir.path().join("runs").join(&plan.run_id);
    plan.pipeline.steps = steps(&run_dir);
    let resume_plan = plan.clone();
    crash_during_group(rig.engine(), plan, &run_dir);
    std::fs::write(run_dir.join("PROCEED"), "go").expect("proceed");
    let outcome = rig.engine().run(&resume_plan).await;
    let events = runlog::read_events(&run_dir).expect("events");
    let seen = (
        count(&events, EventKind::GroupStarted, None),
        member_counts(&events, "fast"),
        member_counts(&events, "block"),
        outcome.outcome,
    );
    assert_eq!(seen, (1, (1, 1), (2, 1), StepOutcome::Success));
}

fn steps(run_dir: &Path) -> Vec<bureau::config::StepDef> {
    let prepare = rig::det_step("prepare", "echo changed >> file.txt", Some("inspect"));
    let mut group = rig::step("inspect", StepKind::Concurrent);
    group.steps = vec!["fast".to_owned(), "block".to_owned()];
    group.completion = Some(Completion::All);
    group.next = Some("done".to_owned());
    let fast = rig::det_step("fast", "printf fast", None);
    let script = format!(
        "touch '{0}/BLOCKING'; while [ ! -f '{0}/PROCEED' ]; do sleep 0.05; done",
        run_dir.display()
    );
    let mut block = rig::det_step("block", &script, None);
    block.max_attempts = 2;
    vec![prepare, group, fast, block]
}

fn crash_during_group(engine: Engine, plan: RunPlan, run_dir: &Path) {
    let run_dir = run_dir.to_path_buf();
    std::thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let observed = run_dir.clone();
        runtime.spawn(async move {
            engine.run(&plan).await;
        });
        runtime.block_on(wait_partial(&observed));
        runtime.shutdown_background();
    })
    .join()
    .expect("crash thread");
}

async fn wait_partial(run_dir: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let events = runlog::read_events(run_dir).unwrap_or_default();
        let fast_done = member_counts(&events, "fast").1 == 1;
        if fast_done && run_dir.join("BLOCKING").exists() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "group never reached partial state"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn member_counts(events: &[Event], member: &str) -> (usize, usize) {
    (
        count(events, EventKind::GroupMemberStarted, Some(member)),
        count(events, EventKind::GroupMemberFinished, Some(member)),
    )
}

fn count(events: &[Event], kind: EventKind, member: Option<&str>) -> usize {
    events
        .iter()
        .filter(|event| {
            event.kind == kind
                && member.is_none_or(|member| event.data["member"].as_str() == Some(member))
        })
        .count()
}

fn use_helpers(rig: &rig::Rig) {
    let result = rig::result(StepOutcome::Success, "unused");
    let fixture = rig::fixture(rig.dir.path(), "unused.json", &result);
    let _ = (
        rig::agent_step("unused", &fixture, None),
        rig::decision_step("unused", "other"),
        Trust::Derived,
    );
}
