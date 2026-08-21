//! The v0 config cutover: plugin-owned models, optional limits, and ADO approval.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::Config;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("bureau-cutover-{suffix}-{tag}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
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
  main:
    url: https://dev.azure.com/acme/project/_git/main
    forge: ado
    access: push
    credential: ado-main
";

const ROLE: &str = r"
name: implementer
agent: /bureau:implementer
adapter: copilot
permissions: [repo:read, repo:write]
min_trust: maintainer
";

const ASSIGNMENT: &str = r#"
name: fix-tests
work:
  forge: ado
  source: "project/main"
  filter: "[System.Tags] CONTAINS 'agent-eligible'"
  abort_label: bureau:failed
  escalate_label: bureau:needs-human
repos: [main]
pipeline: fix-test
role: implementer
verify: "cargo test"
branch_prefix: bureau/
limits:
  max_concurrent: 2
  max_runs_per_hour: 6
  max_runs_per_day: 40
  max_open_prs: 5
  max_cost_per_day_usd: 25
"#;

const PIPELINE: &str = r"
name: fix-test
steps:
  - name: implement
    type: agent
    role: implementer
    next: done
";

fn write(dir: &TestDir, path: &str, text: &str) {
    let path = dir.path().join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
}

fn write_config(dir: &TestDir, role: &str, assignment: &str) {
    write(dir, "repos.yaml", REPOS);
    write(dir, "roles/implementer.yaml", role);
    write(dir, "assignments/fix-tests.yaml", assignment);
    write(dir, "pipelines/fix-test.yaml", PIPELINE);
}

fn errors(dir: &TestDir) -> Vec<String> {
    Config::load(dir.path())
        .expect_err("config must fail")
        .iter()
        .map(ToString::to_string)
        .collect()
}

const fn limits_block() -> &'static str {
    "limits:\n  max_concurrent: 2\n  max_runs_per_hour: 6\n  max_runs_per_day: 40\n  max_open_prs: 5\n  max_cost_per_day_usd: 25\n"
}

#[test]
fn removed_role_fields_have_actionable_errors() {
    let cases = [
        (
            format!("{ROLE}model: opus\n"),
            ("remove `model`", "agent resource"),
        ),
        (
            format!("{ROLE}concurrency: 2\n"),
            ("remove `concurrency`", "max_concurrent"),
        ),
    ];
    for (role, expected) in cases {
        let dir = TestDir::new("removed-role-field");
        write_config(&dir, &role, ASSIGNMENT);
        let found = errors(&dir).join("\n");
        assert!(
            found.contains(expected.0) && found.contains(expected.1),
            "{found}"
        );
    }
}

#[test]
fn limits_block_and_fields_are_optional() {
    let cases = [
        (ASSIGNMENT.replace(limits_block(), ""), (None, None)),
        (
            ASSIGNMENT.replace(limits_block(), "limits:\n  max_cost_per_day_usd: 12.5\n"),
            (None, Some(12.5)),
        ),
    ];
    for (assignment, expected) in cases {
        let dir = TestDir::new("optional-limits");
        let approved = approve(&assignment);
        write_config(&dir, ROLE, &approved);
        let config = Config::load(dir.path()).expect("optional limits");
        let limits = &config.assignments["fix-tests"].limits;
        assert_eq!(
            (limits.max_concurrent, limits.max_cost_per_day_usd),
            expected
        );
    }
}

#[test]
fn ado_agent_requires_an_approval_label() {
    let dir = TestDir::new("ado-approval");
    write_config(&dir, ROLE, ASSIGNMENT);
    let found = errors(&dir).join("\n");
    assert!(
        found.contains("add `work.approval_label`")
            && found.contains("step `implement`")
            && found.contains("role `implementer`"),
        "{found}"
    );
}

#[test]
fn approval_label_makes_the_ado_agent_reachable() {
    let dir = TestDir::new("ado-approved");
    let assignment = approve(ASSIGNMENT);
    write_config(&dir, ROLE, &assignment);
    Config::load(dir.path()).expect("approval label admits agent config");
}

fn approve(assignment: &str) -> String {
    assignment.replace(
        "  source: \"project/main\"\n",
        "  source: \"project/main\"\n  approval_label: agent-approved\n",
    )
}
