//! The `copilot` adapter: runs the GitHub Copilot CLI as the agent.
//!
//! The agent file a developer invokes locally runs unmodified in
//! automation (DESIGN.md section 6): the engine pins and temporarily
//! activates `/plugin:agent` resources before spawn. A direct `.md` path
//! is copied verbatim into `<worktree>/.github/agents/<name>.agent.md`.
//!
//! argv is `copilot -p <request-json> --agent <name> --experimental --sandbox`;
//! the request JSON also arrives on stdin per the layer-2 contract.
//! The push boundary is mirrored in argv (section 10), and a role
//! without a write grant is denied shell outright — the tool grammar
//! matches commands, not effects, so a read-only shell is not
//! expressible:
//!
//! | permissions                   | flags |
//! |-------------------------------|-------|
//! | `repo:write`, not `repo:push` | `--allow-tool=write --allow-tool=shell --add-dir <worktree> --deny-tool='shell(git push)'` |
//! | `repo:push`                   | `--allow-tool=write --allow-tool=shell --add-dir <worktree>` |
//! | anything else                 | `--deny-tool='shell(*)'` |
//!
//! `repo:write` grants editing the run worktree (`write` +
//! `--add-dir <worktree>`), not only the git shell — see issue #16 — and
//! the whole shell, not only `git`, so the agent can run the build and
//! test commands its own instructions tell it to run (DESIGN.md section
//! 10, "shell breadth"). Copilot's MXC sandbox confines that shell and
//! repository policy disables bypass and ambient git/`gh` credentials.
//!
//! Credentials arrive by env convention, gated on the role's grants
//! (section 10): `GH_TOKEN` is a forge credential, forwarded into the
//! child env and added to the scrub list only when the role holds a
//! remote forge grant from [`real::FORGE_GRANTS`]. To record a fixture for the `fake`
//! adapter, run
//! `bureau fake record out.json -- <the argv spawn_request builds>`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::real;
use super::{Execution, Usage};
use crate::config::{Permission, Role, StepDef};
use crate::contract::StepRequest;
use crate::mcp::Session;
use crate::process::{Secret, SharedLog, SpawnRequest};

/// The adapter's working binary name.
pub const BINARY: &str = "copilot";

/// Default per-step timeout when the pipeline does not set one.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(real::DEFAULT_TIMEOUT_SECS);

/// Copilot's agent discovery location inside a worktree.
const DISCOVERY: real::Discovery = real::Discovery {
    dir: ".github/agents",
    suffix: ".agent.md",
};

/// Credential variable forwarded when the role holds a forge grant.
const CREDENTIAL_VARS: [&str; 1] = ["GH_TOKEN"];

/// The `bureau-io` server definition, in the shape this CLI's MCP
/// config actually requires.
///
/// Deliberately not [`real::write_mcp_config`]'s shared shape, which
/// serves `claude`: this CLI spells the transport `local`, not `stdio`,
/// and exposes no tools at all unless the definition carries an explicit
/// filter. A definition missing either is still listed by
/// `copilot mcp list` and still reports `Status: Enabled`, but its tools
/// never reach the model — which is why `--additional-mcp-config` looked
/// like a no-op and a workspace `.mcp.json` looked like the fix.
const MCP_CONFIG: &str = r#"{"mcpServers":{"bureau-io":{"type":"local","command":"bureau","args":["mcp","serve"],"tools":["*"]}}}"#;

/// The push-boundary mirror; see the module table. Without a write
/// grant the CLI gets a deny-by-default flag instead of silence.
///
/// A write grant must allow the agent to *edit* the run worktree, not
/// only run `git` — `shell(git:*)` alone left every `apply_patch`/edit
/// denied (issue #16). So `repo:write` adds the file-edit tool plus
/// an explicit `--add-dir` for the worktree (it sits under `BUREAU_HOME`,
/// outside any repo root the CLI would otherwise verify paths against).
///
/// The shell is granted whole, not just `git`. A step told to validate
/// its own edit needs the repo's build and test commands, and the
/// grantable unit here is the command stem, so an allowlist would have
/// to name every tool a repository happens to use. `git push` stays
/// gated behind `repo:push`: denial beats any allow, so the push
/// boundary survives the wider grant.
fn permission_flags(permissions: &[Permission], worktree: &Path) -> Vec<String> {
    let (write, push) = real::push_boundary(permissions);
    if !write {
        return vec!["--deny-tool=shell(*)".to_owned()];
    }
    let mut flags = vec![
        "--allow-tool=write".to_owned(),
        "--allow-tool=shell".to_owned(),
        "--add-dir".to_owned(),
        worktree.to_string_lossy().into_owned(),
    ];
    if !push {
        flags.push("--deny-tool=shell(git push)".to_owned());
    }
    flags
}

/// `copilot -p <json> --agent <name>` plus the permission mirror.
///
/// The `bureau-io` definition rides on argv so the server the agent is
/// granted is the server that actually launches, and so a recorded
/// fixture reproduces the real invocation.
fn argv(role: &Role, agent: &str, prompt: &[u8], worktree: &Path) -> Vec<String> {
    let mut argv = vec![
        BINARY.to_owned(),
        "-p".to_owned(),
        String::from_utf8_lossy(prompt).into_owned(),
        "--agent".to_owned(),
        agent.to_owned(),
        "--experimental".to_owned(),
        "--sandbox".to_owned(),
        "--allow-tool=bureau-io".to_owned(),
        "--additional-mcp-config".to_owned(),
        MCP_CONFIG.to_owned(),
    ];
    argv.extend(permission_flags(&role.permissions, worktree));
    argv
}

/// Builds the layer-0 request for a `copilot` step: the step contract
/// JSON as `-p` and on stdin, credentials in env, permissions in argv.
///
/// `secrets` is the engine's scrub list; forwarded credential values
/// are appended to it. Building never fails: agent materialization is
/// best effort and the CLI surfaces its own errors at spawn time.
#[must_use]
pub fn spawn_request(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> SpawnRequest {
    let json = request.to_json().unwrap_or_default();
    let agent = real::resolve_agent(&role.agent, &request.worktree, &DISCOVERY);
    let found = real::scoped_credentials(&role.permissions, &real::FORGE_GRANTS, &CREDENTIAL_VARS);
    let (env, secrets) = real::child_env(found, secrets);
    SpawnRequest {
        argv: argv(role, &agent, &json, &request.worktree),
        dir: request.worktree.clone(),
        env,
        stdin: json,
        timeout: real::timeout(step),
        secrets,
        log,
        cancel: None,
    }
}

fn enable_telemetry(env: &mut std::collections::BTreeMap<String, String>, path: &std::path::Path) {
    env.insert("COPILOT_OTEL_ENABLED".to_owned(), "true".to_owned());
    env.insert("COPILOT_OTEL_EXPORTER_TYPE".to_owned(), "file".to_owned());
    env.insert(
        "COPILOT_OTEL_FILE_EXPORTER_PATH".to_owned(),
        path.to_string_lossy().into_owned(),
    );
}

/// Reads the exported telemetry file off the executor's worker threads.
/// A missing or unreadable file is unknown usage, exactly as a failed
/// synchronous read was before.
async fn read_usage(path: std::path::PathBuf) -> Usage {
    let read = tokio::task::spawn_blocking(move || std::fs::read(path)).await;
    match read {
        Ok(Ok(bytes)) => Usage::from_copilot_otel(&bytes),
        _ => Usage::unknown("copilot"),
    }
}

/// Assembles the session and the spawn request.
///
/// The CLI needs the server *definition* to launch it — the
/// `--allow-tool=bureau-io` flag alone references a server it cannot
/// find. [`argv`] carries that definition inline rather than writing it
/// into the run worktree, which would leave an untracked `.mcp.json`
/// for the agent to commit into its own pull request.
fn prepare(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> Result<(Session, PathBuf, SpawnRequest), String> {
    let session = Session::create(request).map_err(|_| "creating bureau-io session failed")?;
    let telemetry = session.dir().join("copilot-otel.jsonl");
    let mut built = spawn_request(role, step, request, secrets, log);
    built.timeout = timeout;
    built.cancel = super::cancel_path(request);
    built.env.extend(session.env().clone());
    enable_telemetry(&mut built.env, &telemetry);
    Ok((session, telemetry, built))
}

/// Runs a `copilot` step and derives the step result.
///
/// A spawn failure is data: it becomes `StepOutcome::Failure` through
/// `result_from_spawn`, never a panic.
#[must_use]
pub async fn execute(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> Execution {
    let (session, telemetry, built) = match prepare(role, step, request, timeout, secrets, log) {
        Ok(prepared) => prepared,
        Err(message) => return super::failed(&message),
    };
    let spawned = crate::process::spawn(built).await;
    let published = match session.published() {
        Ok(result) => result,
        Err(error) => return super::failed(&format!("reading published result failed: {error}")),
    };
    let result = super::result_from_agent(&spawned, published, &spawned.stdout);
    let usage = read_usage(telemetry).await;
    Execution::new(result, usage)
}
