//! The `claude` adapter: runs the Anthropic Claude Code CLI as the
//! agent.
//!
//! Same resource contract as `copilot` (DESIGN.md section 6): the engine
//! pins and temporarily activates `/plugin:agent` before spawn; a direct
//! `.md` path is copied verbatim into `<worktree>/.claude/agents/<name>.md`.
//!
//! argv is `claude -p --agent <name>`: `claude -p`
//! reads the prompt from stdin, which carries the request JSON per the
//! layer-2 contract. The push boundary is mirrored in argv (section
//! 10), one flag per rule, and a role without a write grant is denied
//! shell outright — the tool grammar matches commands, not effects, so
//! a read-only shell is not expressible:
//!
//! | permissions                   | flags |
//! |-------------------------------|-------|
//! | `repo:write`, not `repo:push` | `--allowedTools 'Edit,Write,Bash' --disallowedTools 'Bash(git push:*)'` |
//! | `repo:push`                   | `--allowedTools 'Edit,Write,Bash'` |
//! | anything else                 | `--disallowedTools 'Bash(*)'` |
//!
//! `repo:write` grants editing the run worktree (`Edit`/`Write`), not
//! only the git shell — see issue #16 — and the whole shell, not only
//! `git`, so the agent can run the build and test commands its own
//! instructions tell it to run (DESIGN.md section 10, "shell breadth").
//!
//! Credentials arrive by env convention, gated on the role's grants
//! (section 10): `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` are
//! model credentials, forwarded into the child env and added to the
//! scrub list only when the role holds `model:invoke`
//! ([`real::MODEL_GRANTS`]). To record a fixture for the `fake`
//! adapter, run
//! `bureau fake record out.json -- <the argv spawn_request builds>`.

use super::real;
use super::{Execution, Usage};
use crate::config::{Permission, Role, StepDef};
use crate::contract::StepRequest;
use crate::mcp::Session;
use crate::process::{Secret, SharedLog, SpawnRequest};
use std::time::Duration;

/// The adapter's working binary name.
pub const BINARY: &str = "claude";

/// Claude's agent discovery location inside a worktree.
pub(super) const DISCOVERY: real::Discovery = real::Discovery {
    dir: ".claude/agents",
    suffix: ".md",
};

/// Credential variables forwarded when the role holds `model:invoke`.
const CREDENTIAL_VARS: [&str; 2] = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"];

/// The push-boundary mirror; see the module table. Without a write
/// grant the CLI gets a deny-by-default rule instead of silence.
///
/// A write grant must allow the agent to *edit* the run worktree, not
/// only run `git` — `Bash(git:*)` alone left every edit denied (issue
/// #16). So `repo:write` adds the `Edit` and `Write` tools alongside
/// the shell, granted whole rather than just `git`: a step told to
/// validate its own edit needs the repo's build and test commands
/// (DESIGN.md section 10, "shell breadth"). `git push` stays gated
/// behind `repo:push`, and denial beats any allow.
fn permission_flags(permissions: &[Permission]) -> Vec<String> {
    let (write, push) = real::push_boundary(permissions);
    if !write {
        return vec!["--disallowedTools".to_owned(), "Bash(*)".to_owned()];
    }
    let mut flags = vec!["--allowedTools".to_owned(), "Edit,Write,Bash".to_owned()];
    if !push {
        flags.extend([
            "--disallowedTools".to_owned(),
            "Bash(git push:*)".to_owned(),
        ]);
    }
    flags
}

/// `claude -p --agent <name>` plus the permission mirror.
fn argv(role: &Role, agent: &str) -> Vec<String> {
    let mut argv = vec![
        BINARY.to_owned(),
        "-p".to_owned(),
        "--output-format".to_owned(),
        "json".to_owned(),
        "--agent".to_owned(),
        agent.to_owned(),
        "--allowedTools".to_owned(),
        "mcp__bureau-io__get_step_context,mcp__bureau-io__publish_result".to_owned(),
    ];
    argv.extend(permission_flags(&role.permissions));
    argv
}

/// Builds the layer-0 request for a `claude` step: the step contract
/// JSON on stdin, credentials in env, permissions in argv.
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
    let agent = super::resolved_agent(role, &request.worktree);
    let found = real::scoped_credentials(&role.permissions, &real::MODEL_GRANTS, &CREDENTIAL_VARS);
    let (env, secrets) = real::child_env(found, secrets);
    SpawnRequest {
        argv: argv(role, &agent),
        dir: request.worktree.clone(),
        env,
        stdin: request.to_json().unwrap_or_default(),
        timeout: real::timeout(step),
        secrets,
        log,
        cancel: None,
    }
}

fn prepare(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> Result<(Session, SpawnRequest), String> {
    let session = Session::create(request).map_err(|error| error.to_string())?;
    let config = session.dir().join("mcp.json");
    real::write_mcp_config(&config).map_err(|error| error.to_string())?;
    let mut built = spawn_request(role, step, request, secrets, log);
    built.timeout = timeout;
    built.cancel = super::cancel_path(request);
    built.env.extend(session.env().clone());
    built.argv.extend([
        "--mcp-config".to_owned(),
        config.to_string_lossy().into_owned(),
        "--strict-mcp-config".to_owned(),
    ]);
    Ok((session, built))
}

/// Runs a `claude` step and derives the step result.
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
    let (session, built) = match prepare(role, step, request, timeout, secrets, log) {
        Ok(prepared) => prepared,
        Err(message) => return super::failed(&message),
    };
    let spawned = crate::process::spawn(built).await;
    let response = super::usage::claude_result(&spawned.stdout).unwrap_or_default();
    let published = match session.published() {
        Ok(result) => result,
        Err(error) => return super::failed(&format!("reading published result failed: {error}")),
    };
    let result = super::result_from_agent(&spawned, published, &response);
    let usage = Usage::from_claude_json(&spawned.stdout);
    Execution::new(result, usage)
}
