//! Opt-in evidence stays at its original paths; ordinary test runs remove it.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

use bureau::adapters::copilot_factory::types::FactoryRunStatus;
use bureau::engine::new_run_id;
use bureau::runlog::{EVENTS_FILE, EventKind};
use serde_json::{Value, json};

use super::fixture::{Fixture, lease};

fn fixture(variable: &str, mode: &str) -> (Fixture, bool) {
    std::env::var_os(variable).map_or_else(
        || (Fixture::create(mode), false),
        |directory| (Fixture::at(PathBuf::from(directory), mode), true),
    )
}

fn release(fixture: &Fixture) {
    fixture
        .plan
        .lease
        .as_ref()
        .expect("fixture owner")
        .release()
        .expect("release finished owner");
}

fn next_run(fixture: &mut Fixture) {
    release(fixture);
    fixture.plan.run_id = new_run_id("offline").expect("second Bureau identity");
    fixture.plan.pipeline.steps[0]
        .copilot_factory
        .as_mut()
        .expect("factory")
        .args["mode"] = json!("pause");
    fixture.plan.lease = Some(lease(&fixture.root, &fixture.plan));
}

fn check(fixture: &Fixture, expected: FactoryRunStatus) {
    let record = fixture.record();
    let terminal = usize::from(expected == FactoryRunStatus::Completed);
    assert_eq!(
        (
            record.status(),
            record.execution_clean,
            record.can_clean(),
            record.can_resume(),
            fixture.count(EventKind::StepStarted),
            fixture.count(EventKind::StepFinished),
            fixture.count(EventKind::RunFinished)
        ),
        (
            Some(expected),
            true,
            terminal == 1,
            terminal == 0,
            1,
            terminal,
            terminal
        )
    );
}

fn authentication(fixture: &Fixture) -> Value {
    let trace = fixture.trace();
    let startup = trace
        .iter()
        .find(|entry| entry.get("startupCredential").is_some())
        .expect("actual authenticated startup");
    let descendants: Vec<_> = trace
        .iter()
        .filter(|entry| entry.get("authDescendant").is_some())
        .collect();
    json!({"startup": startup, "descendants": descendants})
}

fn entry(fixture: &Fixture) -> Value {
    let record = fixture.record();
    json!({
        "run_id": fixture.plan.run_id, "directory": fixture.directory(),
        "events": fixture.directory().join(EVENTS_FILE),
        "native_session_id": record.intent.session_id, "native_run_id": record.run_id,
        "native_status": record.status(), "native_attempt": record.attempt,
        "can_start": record.can_start(), "can_resume": record.can_resume(),
        "workspace": record.intent.workspace.directory,
        "model_credential_reference": record.intent.factory.model_credential,
        "authentication": authentication(fixture)
    })
}

async fn execute(fixture: &Fixture, expected: FactoryRunStatus) -> Value {
    let _outcome = fixture.engine.run(&fixture.plan).await;
    check(fixture, expected);
    entry(fixture)
}

fn document(fixture: &Fixture, completed: &Value, paused: &Value) -> Value {
    json!({
        "schema": "bureau-factory-engine-evidence-v1",
        "producer": "copilot_factory_engine::evidence::completed_and_paused_runs",
        "fake_runtime": true, "source": env!("CARGO_MANIFEST_DIR"),
        "runs_dir": fixture.engine.runs_dir, "state_db": fixture.root.join("state.db"),
        "completed": completed, "paused": paused
    })
}

fn save(fixture: &Fixture, document: &Value) {
    let bytes = serde_json::to_vec_pretty(document).expect("evidence JSON");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(fixture.root.join("evidence.json"))
        .expect("new evidence receipt");
    file.write_all(&bytes).expect("write receipt");
    file.sync_all().expect("sync receipt");
    File::open(&fixture.root)
        .expect("evidence directory")
        .sync_all()
        .expect("sync directory");
}

fn finish(mut fixture: Fixture, document: &Value, export: bool) {
    let directory = fixture.root.clone();
    release(&fixture);
    save(&fixture, document);
    if export {
        fixture.retain();
        println!("FACTORY_EVIDENCE={document}");
    }
    drop(fixture);
    assert_eq!(
        directory.exists(),
        export,
        "cleanup must be opt-out only for explicit evidence"
    );
}

#[tokio::test]
async fn completed_and_paused_runs() {
    let (mut fixture, export) = fixture("BUREAU_FACTORY_EVIDENCE_DIR", "success");
    let completed = execute(&fixture, FactoryRunStatus::Completed).await;
    next_run(&mut fixture);
    let paused = execute(&fixture, FactoryRunStatus::Paused).await;
    let document = document(&fixture, &completed, &paused);
    finish(fixture, &document, export);
}

fn check_bootstrap(fixture: &Fixture) {
    let record = fixture.record();
    assert_eq!(
        (
            record.can_start(),
            record.accounting_complete(),
            record.dispatched,
            record.run_id,
            fixture.calls("session.factory.run"),
            fixture.count(EventKind::StepStarted),
            fixture.count(EventKind::StepFinished),
            fixture.directory().join("PAUSE").is_file()
        ),
        (true, false, None, None, 0, 1, 0, true)
    );
}

#[tokio::test]
async fn clean_bootstrap_run() {
    let (fixture, export) = fixture("BUREAU_FACTORY_BOOTSTRAP_EVIDENCE_DIR", "bootstrap-pause");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    check_bootstrap(&fixture);
    let document = json!({
        "schema": "bureau-factory-engine-bootstrap-evidence-v1",
        "producer": "copilot_factory_engine::evidence::clean_bootstrap_run",
        "fake_runtime": true, "source": env!("CARGO_MANIFEST_DIR"),
        "runs_dir": fixture.engine.runs_dir, "state_db": fixture.root.join("state.db"),
        "bootstrap": entry(&fixture)
    });
    finish(fixture, &document, export);
}
