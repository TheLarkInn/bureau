mod fixture;

use bureau::github_cloud::Record;
use clap::Parser;
use serde_json::Value;

use super::{Operation, RunArgs, output};
use crate::cli::{Cli, Verb};
use fixture::Fixture;

const DISPATCH: &[&str] = &["--dispatch-automation", "automation-1"];
const TRACK: &[&str] = &["--automation", "automation-1", "--track-task", "task-1"];

fn args(mode: &[&str]) -> RunArgs {
    let mut args = vec![
        "bureau",
        "run",
        "--github-cloud",
        "--repo",
        "code",
        "--expected-login",
        "runner",
        "--request-id",
        "receipt",
        "--json",
    ];
    args.extend_from_slice(mode);
    let Verb::Run(args) = Cli::try_parse_from(args).expect("cloud CLI arguments").verb else {
        panic!("expected run command");
    };
    args
}

async fn execute(fixture: &Fixture, mode: &[&str]) -> (i32, Value) {
    let args = args(mode);
    let operation = Operation::parse(&args).expect("operation");
    let (state, operation) = operation
        .execute(&fixture.control(), "receipt")
        .await
        .expect("cloud operation");
    let result = output::receipt(&state, operation).expect("receipt output");
    let value = result.value.clone();
    let code = output::finish(Ok(result), args.json).expect("CLI output");
    (code, value)
}

fn status(result: &(i32, Value)) -> &str {
    result.1["submission_status"].as_str().expect("status")
}

#[tokio::test]
async fn creation_only_dispatch_replay_is_not_accepted() {
    let fixture = Fixture::new().await;
    fixture.record(&[]);
    let result = execute(&fixture, DISPATCH).await;
    assert_eq!(
        (result.0, status(&result), fixture.posts()),
        (1, "not_submitted", 0)
    );
}

#[tokio::test]
async fn tracking_only_dispatch_replay_is_not_accepted() {
    let fixture = Fixture::new().await;
    let tracked = execute(&fixture, TRACK).await;
    let replay = execute(&fixture, DISPATCH).await;
    assert_eq!(
        (
            tracked.0,
            status(&tracked),
            replay.0,
            status(&replay),
            fixture.posts()
        ),
        (0, "not_submitted", 1, "not_submitted", 0)
    );
}

#[tokio::test]
async fn accepted_dispatch_replay_succeeds_without_another_post() {
    let fixture = Fixture::new().await;
    fixture.record(&[
        Record::Prepared {
            event: "manual".to_owned(),
        },
        Record::Accepted,
    ]);
    let result = execute(&fixture, DISPATCH).await;
    assert_eq!(
        (result.0, status(&result), fixture.posts()),
        (0, "accepted", 0)
    );
}

#[tokio::test]
async fn not_submitted_remains_successful_for_tracking_and_observation_output() {
    let fixture = Fixture::new().await;
    let state = fixture.record(&[]);
    for operation in ["track", "show", "refresh"] {
        let result = output::receipt(&state, operation).expect("output");
        assert_eq!(
            (result.code, result.value["submission_status"].as_str()),
            (0, Some("not_submitted"))
        );
    }
}
