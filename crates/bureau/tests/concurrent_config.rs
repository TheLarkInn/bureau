//! Static concurrent-group schema and graph-law tests.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{Completion, Config, StepKind};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-concurrent-config-{}-{tag}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("mkdir");
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
    url: https://github.com/acme/main
    forge: github
    access: push
    credential: github-main
";

const ROLE: &str = r"
name: reviewer
agent: /bureau:reviewer
adapter: fake
permissions: [repo:read, model:invoke]
min_trust: untrusted
";

const ASSIGNMENT: &str = r"
name: inspect
work:
  forge: github
  source: acme/main
  filter: is:issue
repos: [main]
pipeline: inspect
role: reviewer
verify: cargo test
branch_prefix: bureau/
";

const PIPELINE: &str = r#"
name: inspect
steps:
  - name: prepare
    type: deterministic
    run: "true"
    next: inspect
  - name: inspect
    type: concurrent
    steps: [tests, review]
    completion: all
    max_concurrent: 2
    next: evaluate
  - name: tests
    type: deterministic
    run: "true"
  - name: review
    type: agent
    role: reviewer
    fixture: "/tmp/review.json"
  - name: evaluate
    type: deterministic
    run: "true"
    inputs_from: [inspect]
    next: done
"#;

fn write_config(dir: &TestDir, role: &str, pipeline: &str) {
    write(dir, "repos.yaml", REPOS);
    write(dir, "roles/reviewer.yaml", role);
    write(dir, "assignments/inspect.yaml", ASSIGNMENT);
    write(dir, "pipelines/inspect.yaml", pipeline);
}

fn write(dir: &TestDir, path: &str, text: &str) {
    let path = dir.path().join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}

fn errors(role: &str, pipeline: &str) -> String {
    let dir = TestDir::new("invalid");
    write_config(&dir, role, pipeline);
    Config::load(dir.path())
        .expect_err("invalid")
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn valid_concurrent_group_loads() {
    let dir = TestDir::new("valid");
    write_config(&dir, ROLE, PIPELINE);
    let config = Config::load(dir.path()).expect("valid concurrent group");
    let group = &config.pipelines["inspect"].steps[1];
    assert_eq!(
        (
            group.kind,
            group.completion,
            group.max_concurrent,
            group.steps.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        (
            StepKind::Concurrent,
            Some(Completion::All),
            Some(2),
            vec!["tests", "review"],
        )
    );
}

#[test]
fn concurrent_member_cannot_own_an_edge_or_receive_one() {
    let cases = [
        (
            PIPELINE.replace(
                "run: \"true\"\n  - name: review",
                "run: \"true\"\n    next: done\n  - name: review",
            ),
            "member cannot have outcome edges",
        ),
        (
            PIPELINE.replace("next: inspect", "next: tests"),
            "edge targets concurrent member `tests`",
        ),
    ];
    for (pipeline, expected) in cases {
        let found = errors(ROLE, &pipeline);
        assert!(found.contains(expected), "{found}");
    }
}

#[test]
fn concurrent_member_cannot_consume_a_sibling() {
    let pipeline = PIPELINE.replace(
        "role: reviewer\n    fixture:",
        "role: reviewer\n    inputs_from: [tests]\n    fixture:",
    );
    let found = errors(ROLE, &pipeline);
    assert!(
        found.contains("cannot consume concurrent sibling `tests`"),
        "{found}"
    );
}

#[test]
fn concurrent_agent_role_is_evidence_only() {
    let role = ROLE.replace("repo:read", "repo:read, repo:write");
    let found = errors(&role, PIPELINE);
    assert!(
        found.contains("concurrent evidence role cannot hold `repo:write`"),
        "{found}"
    );
}

#[test]
fn concurrent_limit_cannot_exceed_members() {
    let pipeline = PIPELINE.replace("max_concurrent: 2", "max_concurrent: 3");
    let found = errors(ROLE, &pipeline);
    assert!(
        found.contains("cannot exceed the number of listed steps"),
        "{found}"
    );
}
