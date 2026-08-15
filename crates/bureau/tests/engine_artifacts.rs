//! Engine-owned artifact confinement, copying, hashing, and scrubbing.

#[path = "engine/rig.rs"]
mod rig;

use std::fs;

use bureau::contract::{Artifact, StepOutcome};
use bureau::process::REDACTED;

#[tokio::test]
async fn published_artifact_is_durable_and_secret_scrubbed() {
    let (outcome, content, named) = publish_artifact().await;
    assert_eq!(
        (
            outcome,
            content.contains("test-credential"),
            content.trim(),
            named
        ),
        (StepOutcome::Success, false, REDACTED, true)
    );
}

async fn publish_artifact() -> (StepOutcome, String, bool) {
    let rig = rig::Rig::new();
    let result = result_with_artifact("report.txt");
    let fixture = rig::fixture(rig.dir.path(), "artifact.json", &result);
    let steps = vec![
        rig::det_step(
            "prepare",
            "printf 'test-credential\\n' > report.txt",
            Some("publish"),
        ),
        rig::agent_step("publish", &fixture, Some("done")),
    ];
    let plan = rig.plan(steps);
    let run_id = plan.run_id.clone();
    let outcome = rig.engine().run(&plan).await.outcome;
    let artifact = run_artifact(&rig, &run_id);
    let content = fs::read_to_string(&artifact).expect("read artifact");
    let named = artifact
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with("-report.txt"));
    (outcome, content, named)
}

fn result_with_artifact(path: &str) -> bureau::contract::StepResult {
    let _unused_fixture_helper = rig::decision_step("unused", "prepare");
    let mut result = rig::result(StepOutcome::Success, "published");
    result.artifacts.push(Artifact {
        name: "report.txt".to_owned(),
        path: path.into(),
    });
    result
}

fn run_artifact(rig: &rig::Rig, run_id: &str) -> std::path::PathBuf {
    only_file(
        &rig.dir
            .path()
            .join("runs")
            .join(run_id)
            .join("artifacts/publish"),
    )
}

#[tokio::test]
async fn artifact_cannot_escape_the_worktree() {
    let cases = [
        ("../outside.txt", "printf x > ../outside.txt"),
        (
            "link.txt",
            "printf x > ../outside.txt && ln -s ../outside.txt link.txt",
        ),
    ];
    for (path, setup) in cases {
        let outcome = run_invalid_artifact(path, setup).await;
        assert_eq!(outcome, StepOutcome::Failure, "{path}");
    }
}

async fn run_invalid_artifact(path: &str, setup: &str) -> StepOutcome {
    let rig = rig::Rig::new();
    let mut result = rig::result(StepOutcome::Success, "invalid");
    result.artifacts.push(Artifact {
        name: "report.txt".to_owned(),
        path: path.into(),
    });
    let fixture = rig::fixture(rig.dir.path(), "invalid.json", &result);
    let steps = vec![
        rig::det_step("prepare", setup, Some("publish")),
        rig::agent_step("publish", &fixture, Some("done")),
    ];
    rig.engine().run(&rig.plan(steps)).await.outcome
}

fn only_file(dir: &std::path::Path) -> std::path::PathBuf {
    let files: Vec<_> = fs::read_dir(dir)
        .expect("artifact dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(files.len(), 1);
    files.into_iter().next().expect("one file")
}
