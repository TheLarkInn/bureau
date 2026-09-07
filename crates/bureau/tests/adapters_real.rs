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

use bureau::adapters::{AdapterKind, claude, copilot, result_from_spawn};
use bureau::config::{Permission, Role, StepDef, StepKind};
use bureau::contract::{SCHEMA_VERSION, StepRequest, Trust, WorkItem};
use bureau::process::{SpawnOutcome, SpawnRequest, SpawnResult};

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
        item: WorkItem::default(),
        trust: Trust::Derived,
        inputs: BTreeMap::new(),
        artifacts: BTreeMap::new(),
    }
}

fn copilot_request(role: &Role, step: &StepDef, dir: &Path) -> SpawnRequest {
    copilot::spawn_request(role, step, &request(dir), Vec::new(), None)
}

#[test]
fn copilot_reserves_stdin_for_acp_and_preserves_native_policy() {
    let dir = TestDir::new("argv");
    let role = role("/no-such-plugin:analyzer", AdapterKind::Copilot, &[]);
    let request = request(dir.path());
    let req = copilot::spawn_request(&role, &step(Some(60)), &request, Vec::new(), None);
    assert_eq!(
        req.argv,
        [
            "copilot",
            "--acp",
            "--stdio",
            "--experimental",
            "--sandbox",
            "--allow-tool=bureau-io",
            "--deny-tool=shell(*)",
        ]
    );
    let shape = (
        req.stdin.is_empty(),
        req.dir == request.worktree,
        req.timeout,
    );
    assert_eq!(shape, (true, true, Duration::from_secs(60)));
}

#[test]
fn unresolvable_plugin_reference_passes_the_name_through() {
    let dir = TestDir::new("plugin");
    let role = role("/plugin-zzz-absent:helper", AdapterKind::Copilot, &[]);
    let req = copilot_request(&role, &step(None), dir.path());
    let copied = dir.path().join(".github/agents/helper.agent.md");
    let seen = (
        bureau::adapters::resolved_agent(&role, dir.path()),
        copied.exists(),
        req.timeout,
    );
    assert_eq!(
        seen,
        (
            "plugin-zzz-absent:helper".to_owned(),
            false,
            copilot::DEFAULT_TIMEOUT
        )
    );
}

#[test]
fn plugin_reference_uses_the_pre_activated_discovery_file() {
    let dir = TestDir::new("plugin-activation");
    let activated = dir.path().join(".github/agents/helper.agent.md");
    std::fs::create_dir_all(activated.parent().expect("parent")).expect("agent dir");
    std::fs::write(&activated, AGENT_BODY).expect("activated agent");
    let role = role("/demo:helper", AdapterKind::Copilot, &[]);
    let agent = bureau::adapters::resolved_agent(&role, dir.path());
    let readback = std::fs::read_to_string(activated).expect("activated agent");
    let seen = (readback.as_str(), agent.as_str());
    assert_eq!(seen, (AGENT_BODY, "demo:helper"));
}

#[test]
fn failed_process_reports_stdout_or_process_error() {
    let result = |stdout: &[u8], error: Option<&str>| SpawnResult {
        outcome: SpawnOutcome::SpawnFailed,
        exit_code: None,
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
        duration: Duration::ZERO,
        error: error.map(str::to_owned),
    };
    let messages = [
        result_from_spawn(&result(b"No such agent: implementer", None)).message,
        result_from_spawn(&result(b"", Some("signal 9"))).message,
    ];
    assert_eq!(messages, ["No such agent: implementer", "signal 9"]);
}

#[test]
fn md_agent_paths_materialize_verbatim_for_both_adapters() {
    let dir = TestDir::new("materialize");
    let agent = dir.path().join("notes.md");
    std::fs::write(&agent, AGENT_BODY).expect("write agent");
    let path = agent.to_str().expect("utf8 path");
    let copilot_role = role(path, AdapterKind::Copilot, &[]);
    let claude_role = role(path, AdapterKind::Claude, &[]);
    let cop = bureau::adapters::resolved_agent(&copilot_role, dir.path());
    let cla = bureau::adapters::resolved_agent(&claude_role, dir.path());
    let read = |p: &str| std::fs::read_to_string(dir.path().join(p)).expect("copy");
    let bodies = (
        read(".github/agents/notes.agent.md"),
        read(".claude/agents/notes.md"),
    );
    let agents = (cop.as_str(), cla.as_str());
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
    let agent = bureau::adapters::resolved_agent(&role, dir.path());
    let copied = dir.path().join(".github/agents/reviewer:v2.agent.md");
    assert_eq!(
        (
            agent.as_str(),
            std::fs::read_to_string(copied).expect("copy"),
        ),
        ("reviewer:v2", AGENT_BODY.to_owned())
    );
}

#[test]
fn claude_uses_the_public_acp_executable_without_cli_prompt_flags() {
    let dir = TestDir::new("claude-stdin");
    let role = role("/p:a", AdapterKind::Claude, &[]);
    let request = request(dir.path());
    let req = claude::spawn_request(&role, &step(Some(5)), &request, Vec::new(), None);
    let shape = (
        req.argv == ["claude-agent-acp"],
        req.stdin.is_empty(),
        req.dir == request.worktree,
    );
    assert_eq!(shape, (true, true, true));
    assert_eq!(req.timeout, Duration::from_secs(5));
}

#[path = "adapters_real/env.rs"]
mod env;
#[path = "adapters_real/identity.rs"]
mod identity;
#[path = "adapters_real/permissions.rs"]
mod permissions;
