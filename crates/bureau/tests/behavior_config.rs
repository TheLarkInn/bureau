//! The config-layer behavior port: goober's gaggle capability tests
//! (python@3.12, dotnet@9, java@21, os=darwin+xcode) refused scheduling
//! without a declared requirement; bureau's equivalents are fail-closed
//! config validation — an assignment referencing anything undeclared is
//! refused with a diagnostic naming it, trust requirements demand an
//! approval label, and the declared verify command is what runs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{Config, StepKind};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-behavior-{}-{}-{tag}",
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

const ASSIGNMENT: &str = r#"
name: fix-flaky-tests
work:
  forge: ado
  source: "Odsp/odsp-web"
  filter: "[System.Tags] CONTAINS 'agent-eligible'"
  approval_label: agent-approved
repos: [odsp-web]
pipeline: fix-failing-test
role: implementer
verify: "rush test --to odsp-web"
branch_prefix: runner/
limits:
  max_concurrent: 2
  max_runs_per_day: 40
"#;

const PIPELINE: &str = r"
name: fix-failing-test
steps:
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
    next: done
    on_failure: propose
";

fn base_files<'a>() -> Vec<(&'static str, &'a str)> {
    vec![
        ("repos.yaml", REPOS),
        ("roles/implementer.yaml", ROLE_IMPLEMENTER),
        ("roles/reviewer.yaml", ROLE_REVIEWER),
        ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
        ("pipelines/fix-failing-test.yaml", PIPELINE),
    ]
}

/// The capability-refusal port (goober's python/dotnet/java scheduling
/// tests): an assignment referencing anything undeclared is refused up
/// front with a diagnostic naming the missing piece, never a cryptic
/// failure at run time.
#[test]
fn an_unsatisfiable_reference_is_refused_with_a_diagnostic() {
    let cases = [
        (
            "pipeline: fix-failing-test",
            "pipeline: no-such",
            "unknown pipeline",
        ),
        ("role: implementer", "role: no-such", "unknown role"),
        ("repos: [odsp-web]", "repos: [no-such]", "unknown repo"),
    ];
    for (from, to, want) in cases {
        refusal_names(from, to, want);
    }
}

/// One refusal case: the assignment with `to` swapped in fails to load,
/// and the diagnostic names both the kind and the missing reference.
fn refusal_names(from: &str, to: &str, want: &str) {
    let dir = TestDir::new("refused");
    let assignment = ASSIGNMENT.replace(from, to);
    let mut files = base_files();
    files[3] = ("assignments/fix-flaky-tests.yaml", &assignment);
    write_files(&dir, &files);
    let found = errors(&dir);
    let named = found
        .iter()
        .any(|e| e.contains(want) && e.contains("no-such"));
    assert!(named, "{to} must name the missing reference: {found:?}");
}

/// The declared-CI-command port (goober's `ApplyGaggleCICommand` tests):
/// the assignment's `verify` string is the command its pipeline's
/// verify step runs — loaded, not hard-coded.
#[test]
fn the_declared_verify_command_loads_onto_the_assignment_and_pipeline() {
    let dir = TestDir::new("verify");
    write_files(&dir, &base_files());
    let config = Config::load(dir.path()).expect("valid config");
    let assignment = &config.assignments["fix-flaky-tests"];
    let verify = config.pipelines["fix-failing-test"]
        .steps
        .iter()
        .find(|s| s.name == "verify")
        .and_then(|s| s.run.clone());
    assert_eq!(
        (assignment.verify.as_str(), verify.as_deref()),
        ("rush test --to odsp-web", Some("rush test --to odsp-web"))
    );
}

/// The host-requirements port (goober's iOS fail-closed claims table):
/// bureau's pre-spawn admission is trust — an ADO item is untrusted
/// until it carries the approval label, so a reachable agent step whose
/// role requires more must declare the label or the config is refused.
#[test]
fn ado_agent_steps_requiring_trust_need_an_approval_label() {
    let dir = TestDir::new("approval");
    let without = ASSIGNMENT.replace("  approval_label: agent-approved\n", "");
    let mut files = base_files();
    files[3] = ("assignments/fix-flaky-tests.yaml", &without);
    write_files(&dir, &files);
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("work.approval_label")),
        "unlabeled ADO admission must name work.approval_label: {found:?}"
    );
}

/// The label that admits the same config, and a GitHub-sourced
/// assignment that never needed one — the sufficient-claims row.
#[test]
fn approved_or_non_ado_configs_load() {
    let dir = TestDir::new("approved");
    write_files(&dir, &base_files());
    let labeled = Config::load(dir.path()).is_ok();
    let github = ASSIGNMENT.replace("forge: ado", "forge: github");
    let github = github.replace("  approval_label: agent-approved\n", "");
    let dir_two = TestDir::new("approved-github");
    let mut files = base_files();
    files[3] = ("assignments/fix-flaky-tests.yaml", &github);
    write_files(&dir_two, &files);
    assert!(
        labeled && Config::load(dir_two.path()).is_ok(),
        "labeled ADO and unlabeled GitHub configs both load"
    );
}

/// The gate-shape port (goober's iOS stage-and-gate assertions): the
/// pipeline's decisions declare a branch per outcome, and the failing
/// branch of the final verdict is an explicit target — never a
/// fall-through.
#[test]
fn the_decision_steps_declare_a_branch_per_outcome() {
    let dir = TestDir::new("decisions");
    write_files(&dir, &base_files());
    let config = Config::load(dir.path()).expect("valid config");
    let pipeline = &config.pipelines["fix-failing-test"];
    let decisions: Vec<_> = pipeline
        .steps
        .iter()
        .filter(|s| s.kind == StepKind::Decision)
        .collect();
    let verdict = decisions
        .iter()
        .find(|s| s.name == "verdict")
        .expect("verdict decision");
    let shape = (
        decisions.len(),
        verdict.over.as_deref(),
        verdict.on.get("failure").map(String::as_str),
        verdict.on.len(),
    );
    assert_eq!(shape, (2, Some("review"), Some("propose"), 4));
}
