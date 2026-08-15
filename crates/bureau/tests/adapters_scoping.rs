//! Permission-scoping tests for the adapters (DESIGN.md section 10).
//!
//! Permissions are checked before spawn: each grant maps to a
//! credential that is or is not injected, and a role without a write
//! grant is denied shell outright. `spawn_request` is pure, so these
//! tests assert on the built [`SpawnRequest`]; the fake-adapter test
//! drives a real replay to prove the run's scrub list reaches it.
//!
//! Reading env is safe; setting it is `unsafe` on edition 2024, so the
//! forwarding assertions adapt to whatever the test process has.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::adapters::fake::{self, Chunk, Stream, Transcript};
use bureau::adapters::{AdapterKind, claude, copilot};
use bureau::config::{Permission, Role, StepDef, StepKind};
use bureau::contract::{SCHEMA_VERSION, StepRequest, Trust};
use bureau::process::{REDACTED, Secret, SpawnRequest};

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

fn role(adapter: AdapterKind, permissions: &[Permission]) -> Role {
    Role {
        name: "reviewer".to_owned(),
        agent: "/no-such-plugin:a".to_owned(),
        adapter,
        permissions: permissions.to_vec(),
        min_trust: Trust::Derived,
    }
}

fn step() -> StepDef {
    StepDef {
        name: "review".to_owned(),
        kind: StepKind::Agent,
        run: None,
        role: Some("reviewer".to_owned()),
        fixture: None,
        trust: None,
        over: None,
        on: BTreeMap::new(),
        next: None,
        on_failure: None,
        on_blocked: None,
        on_no_work: None,
        inputs_from: Vec::new(),
        max_attempts: 1,
        timeout_secs: None,
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

/// Builds a copilot spawn request for a role holding `permissions`.
fn copilot_request(permissions: &[Permission], dir: &Path) -> SpawnRequest {
    let role = role(AdapterKind::Copilot, permissions);
    copilot::spawn_request(&role, &step(), &request(dir), Vec::new(), None)
}

/// Whether the daemon environment holds any of `names`, non-empty.
fn daemon_has(names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
}

/// Grants that forward `GH_TOKEN`: a forge credential unlocks for any
/// repo or PR grant (DESIGN.md section 10).
const FORGE_GRANTS: [Permission; 7] = [
    Permission::RepoRead,
    Permission::RepoWrite,
    Permission::RepoPush,
    Permission::PrRead,
    Permission::PrWrite,
    Permission::PrReview,
    Permission::PrMerge,
];

/// Grants that must not forward `GH_TOKEN`.
const NON_FORGE_GRANTS: [Permission; 4] = [
    Permission::IssuesRead,
    Permission::IssuesWrite,
    Permission::RunsRead,
    Permission::ModelInvoke,
];

#[test]
fn gh_token_forwarding_follows_the_forge_grants() {
    let token = std::env::var("GH_TOKEN").unwrap_or_default();
    let forwarded = |permissions: &[Permission]| {
        let dir = TestDir::new("gh-gate");
        let req = copilot_request(permissions, dir.path());
        req.env.contains_key("GH_TOKEN") && req.secrets.contains(&Secret::new(token.as_str()))
    };
    let seen = (
        FORGE_GRANTS
            .iter()
            .all(|g| forwarded(&[*g]) != token.is_empty()),
        NON_FORGE_GRANTS.iter().any(|g| forwarded(&[*g])),
        forwarded(&[]),
    );
    assert_eq!(seen, (true, false, false));
}

#[test]
fn claude_model_tokens_require_model_invoke() {
    let known = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"];
    let runtime = [
        "PATH",
        "HOME",
        "COPILOT_HOME",
        "CLAUDE_CONFIG_DIR",
        "XDG_CONFIG_HOME",
    ];
    let dir = TestDir::new("model-gate");
    let secrets = vec![Secret::new("engine-secret")];
    let granted = role(AdapterKind::Claude, &[Permission::ModelInvoke]);
    let yes = claude::spawn_request(&granted, &step(), &request(dir.path()), secrets, None);
    let no = claude_request(&[Permission::RepoRead], dir.path());
    let seen = (
        yes.env
            .keys()
            .all(|key| known.contains(&key.as_str()) || runtime.contains(&key.as_str())),
        known.iter().any(|name| yes.env.contains_key(*name)) == daemon_has(&known),
        yes.secrets.contains(&Secret::new("engine-secret")),
        known.iter().all(|name| !no.env.contains_key(*name)),
    );
    assert_eq!(seen, (true, true, true, true));
}

/// Builds a claude spawn request for a role holding `permissions`.
fn claude_request(permissions: &[Permission], dir: &Path) -> SpawnRequest {
    let role = role(AdapterKind::Claude, permissions);
    claude::spawn_request(&role, &step(), &request(dir), Vec::new(), None)
}

/// The fake adapter threads the run's scrub list into the replay
/// spawn: a fixture echoing a credential is redacted before the
/// captured output becomes the step result's message.
#[tokio::test]
async fn fake_replay_scrubs_run_credentials() {
    let dir = TestDir::new("fake-scrub");
    let step = scrub_step(&dir);
    let secrets = vec![Secret::new("cred-123")];
    let result = fake::execute(
        &step,
        &request(dir.path()),
        std::time::Duration::from_secs(300),
        secrets,
        None,
    )
    .await;
    let seen = (
        result.result.message.contains("cred-123"),
        result.result.message.contains(REDACTED),
    );
    assert_eq!(seen, (false, true));
}

fn scrub_step(dir: &TestDir) -> StepDef {
    let fixture = dir.path().join("fixture.json");
    let chunk = Chunk {
        delay_ms: 0,
        stream: Stream::Stderr,
        data: "token=cred-123\n".to_owned(),
    };
    let transcript = Transcript {
        schema: SCHEMA_VERSION.to_owned(),
        chunks: vec![chunk],
        exit_code: 1,
        usage: bureau::adapters::Usage::zero("fake"),
    };
    transcript.save(&fixture).expect("save fixture");
    let mut step = step();
    step.fixture = Some(fixture.to_string_lossy().into_owned());
    step
}
