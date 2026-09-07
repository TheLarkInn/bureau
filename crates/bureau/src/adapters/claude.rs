//! Claude's public ACP adapter. Native grants travel in session metadata,
//! not Claude CLI flags; the shared transport owns the step exchange.

use std::time::Duration;

use super::{Execution, acp, real};
use crate::config::{Role, StepDef};
use crate::contract::StepRequest;
use crate::mcp::Session;
use crate::process::{Secret, SharedLog, SpawnRequest};

/// Executable from the public `@agentclientprotocol/claude-agent-acp` package.
pub const BINARY: &str = "claude-agent-acp";

pub(super) const DISCOVERY: real::Discovery = real::Discovery {
    dir: ".claude/agents",
    suffix: ".md",
};

const CREDENTIAL_VARS: [&str; 2] = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"];

/// Builds the complete, permission-scoped environment for the ACP server.
/// The request itself is delivered by ACP, never as raw stdin or CLI flags.
#[must_use]
pub fn spawn_request(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> SpawnRequest {
    let found = real::scoped_credentials(&role.permissions, &real::MODEL_GRANTS, &CREDENTIAL_VARS);
    let (env, secrets) = real::child_env(found, secrets);
    SpawnRequest {
        argv: vec![BINARY.to_owned()],
        dir: request.worktree.clone(),
        env,
        stdin: Vec::new(),
        timeout: real::timeout(step),
        secrets,
        log,
        cancel: super::cancel_path(request),
    }
}

/// Executes a fresh ACP session with strict Bureau result publication.
#[must_use]
pub async fn execute(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> Execution {
    let session = match Session::create(request) {
        Ok(session) => session,
        Err(error) => return super::failed(&format!("creating bureau-io session failed: {error}")),
    };
    let mut built = spawn_request(role, step, request, secrets, log);
    built.timeout = timeout;
    built.env.extend(session.env().clone());
    acp::execute(role, request, &session, built).await
}
