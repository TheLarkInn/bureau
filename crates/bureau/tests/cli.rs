//! Binary-level tests: `version`, `validate`, and the `fake` adapter
//! testing seam, driven through the built `bureau` binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::adapters::fake::Transcript;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
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

fn bureau(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(args)
        .output()
        .expect("run bureau")
}

fn ok(output: &Output) -> bool {
    output.status.success()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn version_prints_name_and_version() {
    let output = bureau(&["version"]);
    assert!(ok(&output), "{}", stderr(&output));
    assert_eq!(stdout(&output), "bureau 0.1.0\n");
}

#[test]
fn validate_accepts_a_valid_config() {
    let dir = TestDir::new("cli-valid");
    write_minimal_config(dir.path());
    let output = bureau(&["validate", &dir.path().to_string_lossy()]);
    assert!(ok(&output), "{}", stderr(&output));
    assert!(stdout(&output).contains("config ok: 1 repos, 1 roles, 1 assignments"));
}

#[test]
fn validate_reports_every_error_and_fails() {
    let dir = TestDir::new("cli-broken");
    let assignment = MINIMAL_ASSIGNMENT
        .replace("role: worker", "role: ghost")
        .replace("repos: [code]", "repos: [code, missing-repo]");
    write(dir.path(), "repos.yaml", MINIMAL_REPO);
    write(dir.path(), "roles/worker.yaml", MINIMAL_ROLE);
    write(dir.path(), "assignments/demo.yaml", &assignment);
    let output = bureau(&["validate", &dir.path().to_string_lossy()]);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("unknown role `ghost`"), "{err}");
    assert!(err.contains("unknown repo `missing-repo`"), "{err}");
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
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

fn write_minimal_config(dir: &Path) {
    write(dir, "repos.yaml", MINIMAL_REPO);
    write(dir, "roles/worker.yaml", MINIMAL_ROLE);
    write(dir, "assignments/demo.yaml", MINIMAL_ASSIGNMENT);
    write(dir, "pipelines/fix-failing-test.yaml", MINIMAL_PIPELINE);
}

#[test]
fn fake_replay_emits_chunks_and_exit_code() {
    let dir = TestDir::new("cli-replay");
    let fixture = dir.path().join("fixture.json");
    let text = r#"{"schema":"v2","chunks":[
        {"delay_ms":0,"stream":"stdout","data":"hello\n"},
        {"delay_ms":0,"stream":"stderr","data":"oops\n"}],"exit_code":7}"#;
    std::fs::write(&fixture, text).expect("write fixture");
    let output = bureau(&["fake", "replay", &fixture.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(stdout(&output), "hello\n");
    assert_eq!(stderr(&output), "oops\n");
}

fn record_hello(fixture: &Path) -> Output {
    let path = fixture.to_string_lossy().into_owned();
    bureau(&[
        "fake",
        "record",
        &path,
        "--",
        "sh",
        "-c",
        "printf hi; exit 3",
    ])
}

#[test]
fn fake_record_captures_a_replayable_transcript() {
    let dir = TestDir::new("cli-record");
    let fixture = dir.path().join("fixture.json");
    let out = record_hello(&fixture);
    let t = Transcript::load(&fixture).expect("load fixture");
    let recorded = t.chunks.iter().any(|c| c.data == "hi");
    let got = (out.status.code(), stdout(&out), t.exit_code, recorded);
    assert_eq!(got, (Some(3), "hi".to_owned(), 3, true), "{}", stderr(&out));
}

#[test]
fn fake_replay_rejects_a_bad_schema() {
    let dir = TestDir::new("cli-badschema");
    let fixture = dir.path().join("fixture.json");
    std::fs::write(&fixture, r#"{"schema":"v1","chunks":[],"exit_code":0}"#).expect("write");
    let output = bureau(&["fake", "replay", &fixture.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("\"v1\""), "{}", stderr(&output));
}
