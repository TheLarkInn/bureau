use std::process::{Command, Output};

use bureau::runlog::EventKind;
use serde_json::Value;

use super::super::fixture::Fixture;

fn invoke(fixture: &Fixture, verb: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .env_clear()
        .env("BUREAU_HOME", &fixture.root)
        .args([verb, fixture.plan.run_id.as_str(), "--runs"])
        .arg(&fixture.engine.runs_dir)
        .args(extra)
        .output()
        .expect("invoke the actual Bureau CLI")
}

fn check_control(fixture: &Fixture, allowed: bool) {
    let output = invoke(fixture, "show", &["--json"]);
    let value: Value = serde_json::from_slice(&output.stdout).expect("CLI state JSON");
    let control = &value["local_factory_resume"];
    let events = fixture.events();
    let seq = events
        .iter()
        .rfind(|event| event.kind == EventKind::CopilotFactory)
        .expect("native event prefix")
        .seq;
    assert_eq!(
        (
            output.status.success(),
            control["allowed"].as_bool(),
            control["session_id"].as_str(),
            control["event_seq"].as_u64()
        ),
        (
            true,
            Some(allowed),
            Some(fixture.record().intent.session_id.as_str()),
            Some(seq)
        ),
        "{value}",
    );
}

fn resume(fixture: &Fixture) {
    let output = invoke(fixture, "resume", &[]);
    assert_eq!(
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).contains("pause cleared"),
            fixture.directory().join("PAUSE").exists()
        ),
        (true, true, false),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[tokio::test]
async fn actual_cli_continues_a_clean_bootstrap_pause_without_a_second_admission() {
    let fixture = Fixture::create("bootstrap-pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let before = (
        fixture.calls("session.factory.run"),
        fixture.record().run_id,
    );
    check_control(&fixture, true);
    resume(&fixture);
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            before,
            fixture.calls("session.create"),
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::StepStarted),
            fixture.count(EventKind::StepFinished)
        ),
        ((0, None), 1, 1, 0, 1, 1),
    );
}

#[tokio::test]
async fn actual_cli_keeps_ambiguous_admission_paused() {
    let fixture = Fixture::create("ambiguous");
    let _first = fixture.engine.run(&fixture.plan).await;
    std::fs::write(fixture.directory().join("PAUSE"), "operator hold").expect("pause marker");
    check_control(&fixture, false);
    let record = fixture.record();
    let output = invoke(&fixture, "resume", &[]);
    assert_eq!(
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).contains(&record.diagnosis()),
            fixture.directory().join("PAUSE").is_file(),
            fixture.calls("session.factory.run"),
            record.ambiguous_start(),
        ),
        (false, true, true, 1, true),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
}
