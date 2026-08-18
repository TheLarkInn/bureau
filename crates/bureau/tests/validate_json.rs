//! Binary-level tests for `bureau validate --json`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let base = repo_root().join("target").join("bureau-test-work");
        let dir = base.join(format!(
            "validate-json-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn bureau(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(args)
        .output()
        .expect("run bureau")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn validate_json(dir: &Path) -> Output {
    bureau(&["validate", &dir.to_string_lossy(), "--json"])
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("parse validate json")
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
}

#[test]
fn committed_config_json_contains_pipeline_steps_in_order() {
    let output = validate_json(&repo_root().join(".bureau"));
    let value = json(&output);
    let steps = value["config"]["pipelines"]["agent-eligible-pipeline"]["steps"]
        .as_array()
        .expect("steps array");
    let names = step_names(steps);
    let got = (
        output.status.success(),
        stderr(&output),
        value["ok"].as_bool(),
        names,
    );
    assert_eq!(
        got,
        (
            true,
            String::new(),
            Some(true),
            vec!["implement", "verify", "review"]
        )
    );
}

#[test]
fn json_error_line_matches_human_output_and_omits_config() {
    let dir = TestDir::new("broken");
    write_broken_config(dir.path());
    let human = bureau(&["validate", &dir.path().to_string_lossy()]);
    let output = validate_json(dir.path());
    let value = json(&output);
    let error = &value["errors"][0];
    let line = format!("{}: {}", text(&error["path"]), text(&error["message"]));
    let got = (
        line,
        text(&value["dir"]).to_owned(),
        value["config"].is_null(),
        output.status.code(),
    );
    assert_eq!(
        got,
        (
            first_line(&stderr(&human)),
            dir.path().display().to_string(),
            true,
            Some(1)
        )
    );
}

#[test]
fn non_json_validate_output_is_unchanged() {
    let dir = TestDir::new("valid");
    write_valid_config(dir.path());
    let output = bureau(&["validate", &dir.path().to_string_lossy()]);
    let got = (output.status.success(), stdout(&output), stderr(&output));
    assert_eq!(
        got,
        (
            true,
            "config ok: 1 repos, 1 roles, 1 assignments\n".to_owned(),
            String::new()
        )
    );
}

#[test]
fn json_stdout_contains_one_document_only() {
    let output = validate_json(&repo_root().join(".bureau"));
    let mut stream = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Value>();
    let first = stream.next().transpose().expect("first document");
    let second = stream.next().transpose().expect("second document");
    let got = (first.is_some(), second.is_none(), stderr(&output));
    assert_eq!(got, (true, true, String::new()));
}

fn step_names(steps: &[Value]) -> Vec<&str> {
    steps
        .iter()
        .map(|step| text(&step["name"]))
        .collect::<Vec<_>>()
}

fn text(value: &Value) -> &str {
    value.as_str().expect("json string")
}

fn first_line(text: &str) -> String {
    text.lines().next().expect("first stderr line").to_owned()
}

fn write_valid_config(dir: &Path) {
    write(dir, "repos.yaml", MINIMAL_REPO);
    write(dir, "roles/worker.yaml", MINIMAL_ROLE);
    write(dir, "assignments/demo.yaml", MINIMAL_ASSIGNMENT);
    write(dir, "pipelines/fix-failing-test.yaml", MINIMAL_PIPELINE);
}

fn write_broken_config(dir: &Path) {
    let assignment = MINIMAL_ASSIGNMENT.replace("role: worker", "role: ghost");
    write_valid_config(dir);
    write(dir, "assignments/demo.yaml", &assignment);
}

const MINIMAL_REPO: &str = r"
repos:
  code:
    url: https://github.com/example/code
    forge: github
    access: push
    credential: gh-main
";

const MINIMAL_ROLE: &str = r"
name: worker
agent: agents/worker.md
adapter: fake
permissions: [repo:read, repo:write, pr:write]
min_trust: untrusted
";

const MINIMAL_PIPELINE: &str = r#"
name: fix-failing-test
steps:
  - name: work
    type: deterministic
    run: "true"
    next: done
"#;

const MINIMAL_ASSIGNMENT: &str = r#"
name: demo
work:
  forge: github
  source: "example/code"
  filter: "label:agent-eligible"
repos: [code]
pipeline: fix-failing-test
role: worker
verify: "make test"
branch_prefix: runner/
limits:
  max_concurrent: 1
  max_runs_per_hour: 4
  max_runs_per_day: 20
  max_open_prs: 3
  max_cost_per_day_usd: 10
"#;
