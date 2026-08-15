//! Hidden MCP CLI surface tests.

use std::process::{Command, Output};

use bureau::mcp::{BUREAU_STEP_REQUEST, BUREAU_STEP_RESULT};

fn bureau(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bureau"));
    command.args(args).env_clear().envs(env.iter().copied());
    command.output().expect("run bureau")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn normal_help_hides_mcp() {
    let output = bureau(&["--help"], &[]);
    let text = stdout(&output);
    assert_eq!(
        (output.status.success(), text.contains("mcp")),
        (true, false)
    );
}

#[test]
fn nested_help_hides_serve() {
    let output = bureau(&["mcp", "--help"], &[]);
    let text = stdout(&output);
    assert_eq!(
        (output.status.success(), text.contains("serve")),
        (true, false)
    );
}

#[test]
fn serve_reports_each_missing_environment_path() {
    let missing_request = bureau(&["mcp", "serve"], &[]);
    let missing_result = bureau(
        &["mcp", "serve"],
        &[(BUREAU_STEP_REQUEST, "unused-request")],
    );
    let got = (
        missing_request.status.code(),
        stderr(&missing_request).contains(BUREAU_STEP_REQUEST),
        missing_result.status.code(),
        stderr(&missing_result).contains(BUREAU_STEP_RESULT),
    );
    assert_eq!(got, (Some(2), true, Some(2), true));
}
