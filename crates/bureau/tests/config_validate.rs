//! Config loader and `validate` tests (DESIGN.md sections 5–6, 12):
//! every error is reported in one pass.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::Config;

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

const REPOS: &str = r"
repos:
  odsp-web:
    url: https://dev.azure.com/microsoft/Odsp/_git/odsp-web
    forge: ado
    access: push
    credential: ado-main
  augloop:
    url: https://dev.azure.com/office/Augmentation/_git/augloop
    forge: ado
    access: read
    credential: ado-main
";

const ROLE: &str = r"
name: implementer
agent: /bureau:implementer
adapter: copilot
permissions: [repo:read, repo:write, repo:push, pr:write]
min_trust: maintainer
";

const PIPELINE: &str = r#"
name: fix-failing-test
steps:
  - name: work
    type: deterministic
    run: "true"
    next: done
"#;

const ASSIGNMENT: &str = r#"
name: fix-flaky-tests
work:
  forge: ado
  source: "Odsp/odsp-web"
  filter: |
    [System.WorkItemType] = 'Bug'
      AND [System.Tags] CONTAINS 'agent-eligible'
  abort_label: bureau:failed
  escalate_label: bureau:needs-human
repos: [odsp-web, augloop]
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

fn write_files(dir: &TestDir, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
        std::fs::write(path, text).expect("write fixture");
    }
}

const fn valid_files() -> [(&'static str, &'static str); 4] {
    [
        ("repos.yaml", REPOS),
        ("roles/implementer.yaml", ROLE),
        ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
        ("pipelines/fix-failing-test.yaml", PIPELINE),
    ]
}

fn errors(dir: &TestDir) -> Vec<String> {
    Config::load(dir.path())
        .expect_err("config must fail")
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_valid_config_loads() {
    let dir = TestDir::new("valid");
    write_files(&dir, &valid_files());
    let config = Config::load(dir.path()).expect("valid config");
    let counts = (
        config.repos.len(),
        config.roles.len(),
        config.assignments.len(),
    );
    assert_eq!(counts, (2, 1, 1));
    let assignment = &config.assignments["fix-flaky-tests"];
    let primary_and_filter = (
        assignment.primary_repo(),
        assignment.work.filter.contains("agent-eligible"),
    );
    assert_eq!(primary_and_filter, (Some("odsp-web"), true));
}

#[test]
fn missing_repos_yaml_is_an_error() {
    let dir = TestDir::new("norepos");
    write_files(&dir, &[("roles/implementer.yaml", ROLE)]);
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("missing repos.yaml")),
        "{found:?}"
    );
}

#[test]
fn every_error_is_reported_in_one_pass() {
    let dir = TestDir::new("accumulate");
    let assignment = ASSIGNMENT
        .replace("odsp-web, augloop", "nope-web")
        .replace("role: implementer", "role: ghost")
        .replace("max_concurrent: 2", "max_concurrent: 0");
    write_files(
        &dir,
        &[
            ("repos.yaml", REPOS),
            ("assignments/fix-flaky-tests.yaml", &assignment),
        ],
    );
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("unknown repo `nope-web`")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|e| e.contains("unknown role `ghost`")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|e| e.contains("max_concurrent")),
        "{found:?}"
    );
}

#[test]
fn a_read_only_primary_repo_is_an_error() {
    let dir = TestDir::new("readonly");
    let assignment = ASSIGNMENT.replace("repos: [odsp-web, augloop]", "repos: [augloop]");
    write_files(
        &dir,
        &[
            ("repos.yaml", REPOS),
            ("roles/implementer.yaml", ROLE),
            ("assignments/fix-flaky-tests.yaml", &assignment),
        ],
    );
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("branch cannot land")),
        "{found:?}"
    );
}

#[test]
fn a_name_must_match_its_file_stem() {
    let dir = TestDir::new("mismatch");
    write_files(
        &dir,
        &[
            ("repos.yaml", REPOS),
            ("roles/wrong-name.yaml", ROLE),
            ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
        ],
    );
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("does not match file name")),
        "{found:?}"
    );
}

#[test]
fn duplicate_names_are_an_error() {
    let dir = TestDir::new("dupe");
    write_files(
        &dir,
        &[
            ("repos.yaml", REPOS),
            ("roles/implementer.yaml", ROLE),
            ("roles/second.yaml", ROLE),
        ],
    );
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("duplicate name `implementer`")),
        "{found:?}"
    );
}

#[test]
fn an_agent_must_be_a_plugin_invocation_or_a_path() {
    let dir = TestDir::new("agent");
    let role = ROLE.replace("agent: /bureau:implementer", "agent: just-a-name");
    write_files(
        &dir,
        &[("repos.yaml", REPOS), ("roles/implementer.yaml", &role)],
    );
    let found = errors(&dir);
    assert!(
        found.iter().any(|e| e.contains("plugin invocation")),
        "{found:?}"
    );
}

#[test]
fn unknown_fields_are_rejected() {
    let dir = TestDir::new("unknownfield");
    let repos = REPOS.replace(
        "credential: ado-main",
        "credential: ado-main\n    surprise: true",
    );
    write_files(&dir, &[("repos.yaml", &repos)]);
    let found = errors(&dir);
    assert!(found.iter().any(|e| e.contains("surprise")), "{found:?}");
}

#[test]
fn empty_filter_is_an_error() {
    let dir = TestDir::new("emptyfilter");
    let assignment = ASSIGNMENT.replace(
        "  filter: |\n    [System.WorkItemType] = 'Bug'\n      AND [System.Tags] CONTAINS 'agent-eligible'\n",
        "  filter: \"\"\n",
    );
    write_files(
        &dir,
        &[
            ("repos.yaml", REPOS),
            ("roles/implementer.yaml", ROLE),
            ("assignments/fix-flaky-tests.yaml", &assignment),
        ],
    );
    let found = errors(&dir);
    assert!(found.iter().any(|e| e.contains("work.filter")), "{found:?}");
}
