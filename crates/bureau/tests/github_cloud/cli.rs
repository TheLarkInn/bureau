use std::process::{Command, Output};

use bureau::github_cloud::Record;
use serde_json::{Value, json};

use super::control_support::Fixture;
use super::support;

fn invoke(fixture: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .env_clear()
        .env("BUREAU_HOME", fixture.directory())
        .args(args)
        .output()
        .expect("run CLI")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

fn marker_exists(fixture: &Fixture) -> bool {
    ["CANCEL", "PAUSE"]
        .iter()
        .any(|name| fixture.root.join("receipt").join(name).exists())
}

fn prepare(fixture: &Fixture, records: &[Record]) {
    let (owner, mut log) = fixture.log("receipt");
    for record in records {
        log.append(&owner, record).expect("record");
    }
    owner.release().expect("release");
}

fn show_receipt(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec![
        "show",
        "--github-cloud",
        "receipt",
        "--json",
        "--runs",
        fixture.root.to_str().expect("path"),
    ];
    args.extend_from_slice(extra);
    invoke(fixture, &args)
}

const VISIBLE: [&str; 16] = [
    "cancel",
    "dashboard",
    "doctor",
    "fake",
    "init",
    "list",
    "pause",
    "reconcile",
    "repair",
    "resume",
    "retry",
    "run",
    "setup",
    "show",
    "validate",
    "watch",
];

fn commands(help: &str) -> Vec<&str> {
    let (_, body) = help.split_once("Commands:").expect("command section");
    let (body, _) = body.split_once("Options:").expect("option section");
    let mut commands: Vec<_> = body
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter(|line| !line.starts_with(' '))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|command| *command != "help")
        .collect();
    commands.sort_unstable();
    commands
}

fn invalid_runs() -> [Vec<&'static str>; 4] {
    [
        vec!["run", "--github-cloud"],
        vec![
            "run",
            "--github-cloud",
            "--repo",
            "code",
            "--expected-login",
            "runner",
            "--request-id",
            "one",
        ],
        vec![
            "run",
            "pipeline",
            "--github-cloud",
            "--dispatch-automation",
            "automation-1",
        ],
        vec!["run", "--github-cloud", "--track-task", "task-1"],
    ]
}

#[tokio::test]
async fn unsupported_cloud_controls_never_load_credentials_or_write_markers() {
    let fixture = Fixture::new(vec![]).await;
    let (owner, _log) = fixture.log("receipt");
    owner.release().expect("release");
    for verb in ["cancel", "pause", "resume", "retry"] {
        let output = invoke(&fixture, &[verb, "receipt", "--github-cloud", "--json"]);
        let value = json_output(&output);
        assert_eq!(
            (
                output.status.code(),
                value["status"].as_str(),
                value["request_sent"].as_bool(),
                marker_exists(&fixture)
            ),
            (Some(2), Some("unsupported"), Some(false), false)
        );
    }
}

#[tokio::test]
async fn local_control_cannot_mislabel_a_cloud_receipt_as_cancelled() {
    let fixture = Fixture::new(vec![]).await;
    let (owner, _log) = fixture.log("receipt");
    owner.release().expect("release");
    for verb in ["cancel", "pause", "resume"] {
        let output = invoke(
            &fixture,
            &[
                verb,
                "receipt",
                "--runs",
                fixture.root.to_str().expect("path"),
            ],
        );
        assert!(
            output.status.code() == Some(2)
                && text(&output).contains("not a pipeline run")
                && !marker_exists(&fixture)
        );
    }
}

#[tokio::test]
async fn a_cloud_receipt_can_be_read_without_settings_or_credentials() {
    let fixture = Fixture::new(vec![]).await;
    prepare(
        &fixture,
        &[Record::Prepared {
            event: "manual".to_owned(),
        }],
    );
    let output = show_receipt(&fixture, &[]);
    let value = json_output(&output);
    assert_eq!(
        (
            output.status.code(),
            value["submission_status"].as_str(),
            value["remote_controls"].as_str(),
            value["task_correlation"].is_null()
        ),
        (Some(0), Some("uncertain"), Some("unsupported"), true)
    );
}

#[tokio::test]
async fn requested_events_are_not_faked_as_an_empty_complete_snapshot() {
    let fixture = Fixture::new(vec![]).await;
    let (owner, _log) = fixture.log("receipt");
    owner.release().expect("release");
    let output = invoke(
        &fixture,
        &[
            "show",
            "--github-cloud",
            "receipt",
            "--events",
            "--json",
            "--runs",
            fixture.root.to_str().expect("path"),
        ],
    );
    assert!(
        output.status.code() == Some(2) && text(&output).contains("no recorded event snapshot")
    );
}

#[tokio::test]
async fn cloud_run_requires_explicit_mode_identity_and_request_key() {
    let fixture = Fixture::new(vec![]).await;
    for args in invalid_runs() {
        let output = invoke(&fixture, &args);
        assert!(output.status.code() == Some(2) && !text(&output).contains("loading settings"));
    }
}

#[tokio::test]
async fn task_observations_remain_data_even_when_remote_steering_is_true() {
    let fixture = Fixture::new(vec![]).await;
    let mut task = support::task();
    task["remote_steerable"] = json!(true);
    prepare(
        &fixture,
        &[
            Record::TaskSelected {
                task_id: "task-1".to_owned(),
            },
            Record::Observed {
                task,
                events: None,
                reported_total: None,
            },
        ],
    );
    let output = show_receipt(&fixture, &[]);
    let value = json_output(&output);
    assert_eq!(
        (
            value["remote_controls"].as_str(),
            value["receipt"]["task"]["remote_steerable"].as_bool()
        ),
        (Some("unsupported"), Some(true))
    );
}

#[tokio::test]
async fn help_keeps_the_existing_top_level_command_set() {
    let fixture = Fixture::new(vec![]).await;
    let output = invoke(&fixture, &["--help"]);
    let hidden = invoke(&fixture, &["mcp", "--help"]);
    let help = text(&output);
    assert_eq!(
        (
            commands(&help),
            output.status.success(),
            hidden.status.success()
        ),
        (VISIBLE.to_vec(), true, true)
    );
}
