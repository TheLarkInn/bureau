//! Fresh engine evidence for the canvas's run controls, without fabricated events.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;

use bureau::adapters::copilot_factory::types::FactoryRunStatus;
use bureau::contract::StepOutcome;
use bureau::runlog::EventKind;
use serde_json::json;

use super::super::fixture::Fixture;

fn fixture(scenario: &str, mode: &str) -> Fixture {
    std::env::var_os("BUREAU_ENGINE_PAUSE_EVIDENCE_DIR").map_or_else(
        || Fixture::create(mode),
        |root| Fixture::at(PathBuf::from(root).join(scenario), mode),
    )
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn ordinary_pipeline(fixture: &mut Fixture) {
    let home = quote(&fixture.root.join("cli-home").to_string_lossy());
    let executable = quote(env!("CARGO_BIN_EXE_bureau"));
    let run_id = quote(&fixture.plan.run_id);
    let runs = quote(&fixture.engine.runs_dir.to_string_lossy());
    fixture.plan.pipeline = serde_json::from_value(json!({
        "name": "offline", "steps": [{
            "name": "pause-step", "type": "deterministic",
            "run": format!("BUREAU_HOME={home} {executable} pause {run_id} --runs {runs}"),
            "next": "done"
        }]
    }))
    .expect("pipeline invoking the actual CLI pause");
}

async fn run(fixture: &Fixture, expected: StepOutcome) {
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        outcome.outcome,
        expected,
        "{outcome:?}\n{}",
        serde_json::to_string(&fixture.events()).expect("actual engine events")
    );
}

fn export(fixture: &mut Fixture, scenario: &str) {
    if std::env::var_os("BUREAU_ENGINE_PAUSE_EVIDENCE_DIR").is_none() {
        return;
    }
    fixture
        .plan
        .lease
        .as_ref()
        .expect("owner")
        .release()
        .expect("release fixture");
    let document = json!({
        "schema": "bureau-engine-pause-evidence-v1", "scenario": scenario,
        "bureau": env!("CARGO_BIN_EXE_bureau"), "root": fixture.root,
        "run_id": fixture.plan.run_id, "runs_dir": fixture.engine.runs_dir,
        "events": fixture.directory().join("events.jsonl")
    });
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(fixture.root.join("evidence.json"))
        .expect("new actual engine evidence");
    file.write_all(&serde_json::to_vec_pretty(&document).expect("evidence JSON"))
        .expect("write evidence");
    file.sync_all().expect("sync evidence");
    fixture.retain();
}

#[tokio::test]
async fn ordinary_cli_pause() {
    let mut fixture = fixture("ordinary", "success");
    ordinary_pipeline(&mut fixture);
    run(&fixture, StepOutcome::NoWork).await;
    export(&mut fixture, "ordinary");
    assert_eq!(
        (
            std::fs::read_to_string(fixture.directory().join("PAUSE")).expect("CLI pause marker"),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished),
            fixture.count(EventKind::CopilotFactory)
        ),
        ("paused\n".into(), 1, 0, 0)
    );
}

#[tokio::test]
async fn clean_bootstrap_pause() {
    let mut fixture = fixture("bootstrap", "bootstrap-pause");
    run(&fixture, StepOutcome::NoWork).await;
    export(&mut fixture, "bootstrap");
    let record = fixture.record();
    assert_eq!(
        (
            record.can_start(),
            record.run_id.as_deref(),
            record.status(),
            fixture.calls("session.factory.run"),
            fixture.directory().join("PAUSE").is_file(),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (true, None, None, 0, true, 0, 0)
    );
}

#[tokio::test]
async fn sdk_paused_run() {
    let mut fixture = fixture("sdk-paused", "pause");
    run(&fixture, StepOutcome::NoWork).await;
    export(&mut fixture, "sdk-paused");
    let record = fixture.record();
    assert_eq!(
        (
            record.status(),
            record.can_resume(),
            fixture.calls("session.factory.run"),
            fixture.directory().join("PAUSE").is_file(),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (Some(FactoryRunStatus::Paused), true, 1, true, 0, 0)
    );
}

#[tokio::test]
async fn completed_run() {
    let mut fixture = fixture("finished", "success");
    // The completed fixture leaves its repository unchanged.
    run(&fixture, StepOutcome::NoWork).await;
    export(&mut fixture, "finished");
    assert_eq!(
        (
            fixture.record().status(),
            fixture.calls("session.factory.run"),
            fixture.directory().join("PAUSE").exists(),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (Some(FactoryRunStatus::Completed), 1, false, 1, 1)
    );
}
