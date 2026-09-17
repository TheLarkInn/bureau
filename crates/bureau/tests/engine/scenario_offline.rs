//! A newcomer can run the shipped design-review pipeline with entirely local effects.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use bureau::config::{AdapterKind, Config, Pipeline, Role, StepKind};
use bureau::contract::{StepOutcome, Trust};
use bureau::engine::{RunOutcome, RunPlan};
use bureau::forge::Forge as _;
use bureau::runlog::{self, EventKind};

use super::rig::{Rig, det_step, fixture, result};

fn scenario_root(id: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/scenarios")
        .join(id)
}

fn offline_roles(roles: &mut BTreeMap<String, Role>) {
    for role in roles.values_mut() {
        role.adapter = AdapterKind::Fake;
        role.agent = format!("agents/{}.md", role.name);
    }
}

fn offline_steps(id: &str, pipeline: &mut Pipeline) {
    for step in &mut pipeline.steps {
        if step.kind == StepKind::Agent {
            let path = scenario_root(id)
                .join("fixtures")
                .join(format!("{}.json", step.name));
            step.fixture = Some(path.to_string_lossy().into_owned());
        }
    }
}

fn example_plan(rig: &Rig, id: &str) -> RunPlan {
    let mut config = Config::load(&scenario_root(id)).expect("shipped config");
    offline_roles(&mut config.roles);
    let assignment = config.assignments.remove(id).expect("assignment");
    let mut pipeline = config
        .pipelines
        .remove(&assignment.pipeline)
        .expect("pipeline");
    offline_steps(id, &mut pipeline);
    let primary = config.repos.get_mut("code").expect("primary");
    primary.url.clone_from(&rig.url);
    "git-main".clone_into(&mut primary.credential);
    let mut plan = rig.plan(Vec::new());
    plan.assignment = assignment;
    plan.pipeline = pipeline;
    plan.roles = config.roles;
    plan.repos = config.repos;
    plan.item.trust = Trust::Maintainer;
    plan.item.labels = plan
        .assignment
        .work
        .approval_label
        .iter()
        .cloned()
        .collect();
    plan
}

async fn approve(rig: &Rig, plan: &RunPlan) {
    rig.forge
        .set_labels(&plan.item.external_id, &plan.item.labels)
        .await
        .expect("fake maintainer approval");
}

async fn design_review() -> (Rig, RunPlan, RunOutcome) {
    let rig = Rig::new();
    let plan = example_plan(&rig, "design-review");
    approve(&rig, &plan).await;
    let outcome = rig.engine().run(&plan).await;
    (rig, plan, outcome)
}

fn finished_steps(rig: &Rig, run_id: &str) -> Vec<String> {
    let events = runlog::read_events(&rig.dir.path().join("runs").join(run_id)).expect("events");
    events
        .into_iter()
        .filter(|event| event.kind == EventKind::StepFinished)
        .map(|event| event.data["step"].as_str().expect("step name").to_owned())
        .collect()
}

fn report_from_local_push(rig: &Rig, outcome: &RunOutcome) -> serde_json::Value {
    let branch = &outcome.pr.as_ref().expect("fake PR").branch;
    let output = Command::new("git")
        .args(["show", &format!("{branch}:docs/reviews/design-review.json")])
        .current_dir(&rig.url)
        .output()
        .expect("read local pushed report");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("checked report")
}

#[tokio::test]
async fn design_review_demo_runs_offline_end_to_end() {
    let (rig, _plan, outcome) = design_review().await;
    assert_eq!(outcome.outcome, StepOutcome::Success, "{}", outcome.message);
    let report = report_from_local_push(&rig, &outcome);
    assert_eq!(
        (
            outcome.cost_usd.to_bits(),
            report["recommendation"].as_str(),
            finished_steps(&rig, &outcome.run_id),
        ),
        (
            0.0_f64.to_bits(),
            Some("revise"),
            ["baseline", "analyze", "render", "review", "verify"]
                .map(str::to_owned)
                .to_vec()
        )
    );
}

#[tokio::test]
async fn offline_demo_replay_does_not_publish_twice() {
    let (rig, plan, first) = design_review().await;
    let replayed = rig.engine().run(&plan).await;
    let prs = rig
        .forge
        .open_prs(&rig.url, &plan.assignment.branch_prefix)
        .await
        .expect("PRs");
    assert_eq!(
        (first.outcome, replayed.outcome, prs.len()),
        (StepOutcome::Success, StepOutcome::Success, 1)
    );
}

#[tokio::test]
async fn invalid_offline_report_cannot_publish() {
    let rig = Rig::new();
    let mut plan = example_plan(&rig, "design-review");
    let invalid = fixture(
        rig.dir.path(),
        "invalid.json",
        &result(StepOutcome::Success, "no report"),
    );
    plan.pipeline.steps[1].fixture = Some(invalid);
    approve(&rig, &plan).await;
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (
            outcome.outcome,
            outcome.pr.is_none(),
            finished_steps(&rig, &outcome.run_id)
        ),
        (
            StepOutcome::Failure,
            true,
            ["baseline", "analyze", "render"]
                .map(str::to_owned)
                .to_vec()
        )
    );
}

#[tokio::test]
async fn design_verification_catches_already_checkpointed_code_edits() {
    let rig = Rig::new();
    let mut plan = example_plan(&rig, "design-review");
    plan.pipeline.steps[1].next = Some("unexpected-edit".to_owned());
    let edit = det_step(
        "unexpected-edit",
        "printf changed > file.txt",
        Some("render"),
    );
    plan.pipeline.steps.insert(2, edit);
    approve(&rig, &plan).await;
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (
            outcome.outcome,
            outcome.pr.is_none(),
            finished_steps(&rig, &outcome.run_id).into_iter().last(),
        ),
        (StepOutcome::Failure, true, Some("verify".to_owned()))
    );
}

#[tokio::test]
async fn unqualified_factory_example_never_runs_an_agent() {
    let rig = Rig::new();
    let plan = example_plan(&rig, "local-sdk-factory");
    approve(&rig, &plan).await;
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (
            outcome.outcome,
            outcome.pr.is_none(),
            finished_steps(&rig, &outcome.run_id)
        ),
        (
            StepOutcome::Blocked,
            true,
            vec!["require-qualification".to_owned()]
        ),
        "{}",
        outcome.message
    );
}
