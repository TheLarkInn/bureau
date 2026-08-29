//! Pipeline config tests (DESIGN.md sections 7, 11): the reference
//! `fix-failing-test` pipeline loads, and every validation rule fails
//! closed with the offending step named.

#[path = "pipeline/terminal_names.rs"]
mod terminal_names;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{Config, StepKind};

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

fn write_files(dir: &TestDir, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
        std::fs::write(path, text).expect("write fixture");
    }
}

fn errors(dir: &TestDir) -> Vec<String> {
    Config::load(dir.path())
        .expect_err("config must fail")
        .iter()
        .map(ToString::to_string)
        .collect()
}

const REPOS: &str = "repos:\n  odsp-web:\n    url: https://dev.azure.com/microsoft/Odsp/_git/odsp-web\n    forge: ado\n    access: push\n    credential: ado-main\n";
const ROLE_IMPLEMENTER: &str = "name: implementer\nagent: /bureau:implementer\nadapter: copilot\npermissions: [repo:read, repo:write, repo:push, pr:write]\nmin_trust: maintainer\n";
const ROLE_REVIEWER: &str = "name: reviewer\nagent: /bureau:reviewer\nadapter: copilot\npermissions: [repo:read, pr:write]\nmin_trust: derived\n";
const ROLE_FAKE: &str = "name: test-double\nagent: /testing:double\nadapter: fake\npermissions: [repo:read]\nmin_trust: untrusted\n";

const ASSIGNMENT: &str = r#"
name: fix-flaky-tests
work:
  forge: ado
  source: "Odsp/odsp-web"
  filter: "[System.Tags] CONTAINS 'agent-eligible'"
  approval_label: agent-approved
  abort_label: bureau:failed
  escalate_label: bureau:needs-human
repos: [odsp-web]
pipeline: fix-failing-test
role: implementer
verify: "rush test --to odsp-web"
branch_prefix: runner/
limits:
  max_concurrent: 2
  max_runs_per_hour: 6
  max_runs_per_day: 40
  max_open_prs: 5
  max_cost_per_day_usd: 25
"#;

// The DESIGN.md section 11 reference pipeline, in the `StepDef` shape.
const PIPELINE: &str = r"
name: fix-failing-test
steps:
  - name: claim
    type: deterministic
    run: bureau claim --next
    next: reproduce
    on_no_work: done
  - name: reproduce
    type: deterministic
    run: scripts/reproduce.sh
    next: propose
  - name: propose
    type: agent
    role: implementer
    inputs_from: [reproduce]
    max_attempts: 3
    next: apply
    on_failure: escalate
    on_blocked: escalate
  - name: apply
    type: deterministic
    run: scripts/apply-and-rerun.sh
    next: passed
    on_failure: escalate
  - name: passed
    type: decision
    over: apply
    on: {success: review, failure: propose, blocked: escalate, no-work: abort}
  - name: review
    type: agent
    role: reviewer
    inputs_from: [apply]
    next: verdict
    on_failure: escalate
  - name: verdict
    type: decision
    over: review
    on: {success: verify, failure: propose, blocked: escalate, no-work: abort}
  - name: verify
    type: deterministic
    run: rush test --to odsp-web
    next: publish
    on_failure: propose
  - name: publish
    type: deterministic
    run: scripts/publish.sh
    next: done
    on_failure: escalate
";

const FIXTURE_PIPELINE: &str = "name: fixture-run\nsteps:\n  - name: propose\n    type: agent\n    role: test-double\n    fixture: /fixtures/propose.jsonl\n    next: done\n";

fn base_files(pipeline: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("repos.yaml", REPOS),
        ("roles/implementer.yaml", ROLE_IMPLEMENTER),
        ("roles/reviewer.yaml", ROLE_REVIEWER),
        ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
        ("pipelines/fix-failing-test.yaml", pipeline),
    ]
}

fn fixture_files(fixture_pipeline: &str) -> Vec<(&'static str, &str)> {
    let mut files = base_files(PIPELINE);
    files.push(("roles/test-double.yaml", ROLE_FAKE));
    files.push(("pipelines/fixture-run.yaml", fixture_pipeline));
    files
}

#[test]
fn the_reference_pipeline_loads() {
    let dir = TestDir::new("reference");
    write_files(&dir, &base_files(PIPELINE));
    let config = Config::load(dir.path()).expect("valid config");
    let pipeline = &config.pipelines["fix-failing-test"];
    let shape = (
        pipeline.steps.len(),
        pipeline.steps[4].kind,
        config.roles.len(),
    );
    assert_eq!(shape, (9, StepKind::Decision, 2));
}

#[test]
fn a_decision_missing_a_branch_is_an_error() {
    let dir = TestDir::new("missingbranch");
    let pipeline = PIPELINE.replace(", no-work: abort", "");
    write_files(&dir, &base_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("step `passed`") && e.contains("no-work")),
        "{found:?}"
    );
}

#[test]
fn an_unknown_edge_target_is_an_error() {
    let dir = TestDir::new("unknownedge");
    let pipeline = PIPELINE.replace("next: reproduce", "next: nowhere");
    write_files(&dir, &base_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("unknown step `nowhere`")),
        "{found:?}"
    );
}

#[test]
fn old_join_vocabulary_is_rejected() {
    let dir = TestDir::new("join");
    let pipeline = PIPELINE.replace("next: done", "next: join");
    write_files(&dir, &base_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("targets unknown step `join`")),
        "{found:?}"
    );
}

#[test]
fn a_fixture_on_a_real_adapter_is_an_error() {
    let dir = TestDir::new("realfixture");
    let pipeline = FIXTURE_PIPELINE.replace("role: test-double", "role: implementer");
    write_files(&dir, &fixture_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("fake` adapter")),
        "{found:?}"
    );
}

#[test]
fn a_relative_fixture_is_an_error() {
    let dir = TestDir::new("relfixture");
    let pipeline = FIXTURE_PIPELINE.replace("/fixtures/propose.jsonl", "fixtures/propose.jsonl");
    write_files(&dir, &fixture_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("absolute path")),
        "{found:?}"
    );
}

#[test]
fn a_fake_adapter_with_an_absolute_fixture_loads() {
    let dir = TestDir::new("fakefixture");
    write_files(&dir, &fixture_files(FIXTURE_PIPELINE));
    let config = Config::load(dir.path()).expect("valid config");
    let step = &config.pipelines["fixture-run"].steps[0];
    assert_eq!(step.fixture.as_deref(), Some("/fixtures/propose.jsonl"));
}

#[test]
fn a_forward_inputs_from_is_an_error() {
    let dir = TestDir::new("forwardinputs");
    let pipeline = PIPELINE.replace("inputs_from: [reproduce]", "inputs_from: [apply]");
    write_files(&dir, &base_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("`apply` is not an earlier step")),
        "{found:?}"
    );
}

#[test]
fn an_unreachable_step_is_an_error() {
    let dir = TestDir::new("unreachable");
    let pipeline = format!("{PIPELINE}  - name: orphan\n    type: deterministic\n    run: true\n");
    write_files(&dir, &base_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("step `orphan`") && e.contains("unreachable")),
        "{found:?}"
    );
}

#[test]
fn an_unknown_assignment_pipeline_is_an_error() {
    let dir = TestDir::new("unknownpipeline");
    let assignment = ASSIGNMENT.replace("pipeline: fix-failing-test", "pipeline: no-such");
    let mut files = base_files(PIPELINE);
    files[3] = ("assignments/fix-flaky-tests.yaml", assignment.as_str());
    write_files(&dir, &files);
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("unknown pipeline `no-such`")),
        "{found:?}"
    );
}

#[test]
fn a_field_on_the_wrong_kind_is_an_error() {
    let dir = TestDir::new("wrongkind");
    let pipeline = PIPELINE.replace(
        "    inputs_from: [reproduce]",
        "    run: scripts/nope.sh\n    inputs_from: [reproduce]",
    );
    write_files(&dir, &base_files(&pipeline));
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("step `propose`") && e.contains("does not apply")),
        "{found:?}"
    );
}
