//! Real-adapter (`copilot`, `claude`) tests.
//!
//! `spawn_request` is pure — it builds a [`SpawnRequest`] without
//! spawning — so these tests assert on the request: argv, env, stdin,
//! and direct-path agent materialization. `execute` is not smoke-tested
//! here: a stub binary on `PATH` would need `std::env::set_var`,
//! which is `unsafe` on edition 2024 and forbidden in this workspace.
//! The `fake` adapter covers the spawn path end to end.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bureau::adapters::{AdapterKind, claude, copilot};
use bureau::config::{Permission, Role, StepDef, StepKind};
use bureau::contract::{SCHEMA_VERSION, StepRequest, Trust};
use bureau::process::SpawnRequest;

/// Joins argv for one-line comparisons (unit separator).
const SEP: &str = "\u{1f}";

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

const AGENT_BODY: &str = "---\nname: helper\ndescription: test\n---\nYou help.\n";

fn role(agent: &str, adapter: AdapterKind, permissions: &[Permission]) -> Role {
    Role {
        name: "reviewer".to_owned(),
        agent: agent.to_owned(),
        adapter,
        permissions: permissions.to_vec(),
        min_trust: Trust::Derived,
    }
}

fn step(timeout_secs: Option<u64>) -> StepDef {
    StepDef {
        name: "review".to_owned(),
        kind: StepKind::Agent,
        run: None,
        role: Some("reviewer".to_owned()),
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        steps: Vec::new(),
        completion: None,
        max_concurrent: None,
        next: None,
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs,
    }
}

fn request(worktree: &Path) -> StepRequest {
    StepRequest {
        schema: SCHEMA_VERSION.to_owned(),
        run_id: "run-1".to_owned(),
        step: "review".to_owned(),
        worktree: worktree.to_path_buf(),
        trust: Trust::Derived,
        inputs: BTreeMap::new(),
        artifacts: BTreeMap::new(),
    }
}

fn copilot_request(role: &Role, step: &StepDef, dir: &Path) -> SpawnRequest {
    copilot::spawn_request(role, step, &request(dir), Vec::new(), None)
}

fn claude_request(role: &Role, step: &StepDef, dir: &Path) -> SpawnRequest {
    claude::spawn_request(role, step, &request(dir), Vec::new(), None)
}

/// The argv value following `flag`.
fn value_after<'a>(argv: &'a [String], flag: &str) -> &'a str {
    let at = argv.iter().position(|a| a == flag).expect("flag present");
    &argv[at + 1]
}

/// Asserts copilot's argv carries the JSON prompt and selected agent.
/// This grant-less role also gets the deny-by-default flag, so argv
/// has 7 elements including the bureau-io grant.
fn assert_copilot_argv(req: &SpawnRequest, json: &str) {
    let parts = (
        req.argv.first().map(String::as_str),
        value_after(&req.argv, "-p"),
        value_after(&req.argv, "--agent"),
        req.argv.len(),
    );
    let expected = (Some(copilot::BINARY), json, "analyzer", 7);
    assert_eq!(parts, expected);
}

#[test]
fn copilot_passes_request_json_as_prompt_and_stdin() {
    let dir = TestDir::new("argv");
    let role = role("/no-such-plugin:analyzer", AdapterKind::Copilot, &[]);
    let request = request(dir.path());
    let req = copilot::spawn_request(&role, &step(Some(60)), &request, Vec::new(), None);
    let json = String::from_utf8(request.to_json().expect("serialize")).expect("utf8");
    let stdin = StepRequest::from_json(&req.stdin).expect("stdin parses");
    assert_copilot_argv(&req, &json);
    let shape = (stdin == request, req.dir == request.worktree, req.timeout);
    assert_eq!(shape, (true, true, Duration::from_secs(60)));
}

#[test]
fn unresolvable_plugin_reference_passes_the_name_through() {
    let dir = TestDir::new("plugin");
    let role = role("/plugin-zzz-absent:helper", AdapterKind::Copilot, &[]);
    let req = copilot_request(&role, &step(None), dir.path());
    let copied = dir.path().join(".github/agents/helper.agent.md");
    let seen = (
        value_after(&req.argv, "--agent"),
        copied.exists(),
        req.timeout,
    );
    assert_eq!(seen, ("helper", false, copilot::DEFAULT_TIMEOUT));
}

#[test]
fn plugin_reference_uses_the_pre_activated_discovery_file() {
    let dir = TestDir::new("plugin-activation");
    let activated = dir.path().join(".github/agents/helper.agent.md");
    std::fs::create_dir_all(activated.parent().expect("parent")).expect("agent dir");
    std::fs::write(&activated, AGENT_BODY).expect("activated agent");
    let role = role("/demo:helper", AdapterKind::Copilot, &[]);
    let req = copilot_request(&role, &step(None), dir.path());
    let readback = std::fs::read_to_string(activated).expect("activated agent");
    let seen = (readback.as_str(), value_after(&req.argv, "--agent"));
    assert_eq!(seen, (AGENT_BODY, "helper"));
}

#[test]
fn md_agent_paths_materialize_verbatim_for_both_adapters() {
    let dir = TestDir::new("materialize");
    let agent = dir.path().join("notes.md");
    std::fs::write(&agent, AGENT_BODY).expect("write agent");
    let path = agent.to_str().expect("utf8 path");
    let copilot_role = role(path, AdapterKind::Copilot, &[]);
    let claude_role = role(path, AdapterKind::Claude, &[]);
    let cop = copilot_request(&copilot_role, &step(None), dir.path());
    let cla = claude_request(&claude_role, &step(None), dir.path());
    let read = |p: &str| std::fs::read_to_string(dir.path().join(p)).expect("copy");
    let bodies = (
        read(".github/agents/notes.agent.md"),
        read(".claude/agents/notes.md"),
    );
    let agents = (
        value_after(&cop.argv, "--agent"),
        value_after(&cla.argv, "--agent"),
    );
    let seen = (bodies.0 == AGENT_BODY, bodies.1 == AGENT_BODY, agents);
    assert_eq!(seen, (true, true, ("notes", "notes")));
}

#[cfg(unix)]
#[test]
fn absolute_agent_path_with_colon_remains_a_path() {
    let dir = TestDir::new("colon-path");
    let agent = dir.path().join("reviewer:v2.md");
    std::fs::write(&agent, AGENT_BODY).expect("write agent");
    let role = role(
        agent.to_str().expect("utf8 path"),
        AdapterKind::Copilot,
        &[],
    );
    let request = copilot_request(&role, &step(None), dir.path());
    let copied = dir.path().join(".github/agents/reviewer:v2.agent.md");
    assert_eq!(
        (
            value_after(&request.argv, "--agent"),
            std::fs::read_to_string(copied).expect("copy"),
        ),
        ("reviewer:v2", AGENT_BODY.to_owned())
    );
}

#[test]
fn claude_carries_the_request_on_stdin_only() {
    let dir = TestDir::new("claude-stdin");
    let role = role("/p:a", AdapterKind::Claude, &[]);
    let request = request(dir.path());
    let req = claude::spawn_request(&role, &step(Some(5)), &request, Vec::new(), None);
    let stdin = StepRequest::from_json(&req.stdin).expect("stdin parses");
    let argv = [
        claude::BINARY,
        "-p",
        "--output-format",
        "json",
        "--agent",
        "a",
        "--allowedTools",
        "mcp__bureau-io__get_step_context,mcp__bureau-io__publish_result",
        "--disallowedTools",
        "Bash(*)",
    ]
    .join(SEP);
    let shape = (
        req.argv.join(SEP) == argv,
        stdin == request,
        req.dir == request.worktree,
    );
    assert_eq!(shape, (true, true, true));
    assert_eq!(req.timeout, Duration::from_secs(5));
}

#[test]
fn copilot_mirrors_the_push_boundary_and_denies_by_default() {
    let edit = "--allow-tool=write\u{1f}--allow-tool=shell(git:*)\u{1f}--allow-all-paths";
    let cases: [(&[Permission], String); 4] = [
        (
            &[Permission::RepoWrite],
            format!("{edit}\u{1f}--deny-tool=shell(git push)"),
        ),
        (&[Permission::RepoPush], edit.to_owned()),
        (&[Permission::RepoRead], "--deny-tool=shell(*)".to_owned()),
        (&[], "--deny-tool=shell(*)".to_owned()),
    ];
    for (permissions, flags) in cases {
        let dir = TestDir::new("copilot-flags");
        let role = role("/p:a", AdapterKind::Copilot, permissions);
        let req = copilot_request(&role, &step(None), dir.path());
        assert_eq!(req.argv[6..].join(SEP), flags);
    }
}

#[test]
fn claude_mirrors_the_push_boundary_and_denies_by_default() {
    let edit = "--allowedTools\u{1f}Edit,Write,Bash(git:*)";
    let cases: [(&[Permission], String); 4] = [
        (
            &[Permission::RepoWrite],
            format!("{edit}\u{1f}--disallowedTools\u{1f}Bash(git push:*)"),
        ),
        (&[Permission::RepoPush], edit.to_owned()),
        (
            &[Permission::RepoRead],
            "--disallowedTools\u{1f}Bash(*)".to_owned(),
        ),
        (&[], "--disallowedTools\u{1f}Bash(*)".to_owned()),
    ];
    for (permissions, flags) in cases {
        let dir = TestDir::new("claude-flags");
        let role = role("/p:a", AdapterKind::Claude, permissions);
        let req = claude_request(&role, &step(None), dir.path());
        assert_eq!(req.argv[8..].join(SEP), flags);
    }
}

#[path = "adapters_real/env.rs"]
mod env;
