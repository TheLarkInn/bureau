//! Copilot's stdio ACP server with the existing native sandbox and role grants.
//! ACP changes the transport, not the credential or command permission boundary.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Execution, Usage, acp, real};
use crate::config::{Permission, Role, StepDef};
use crate::contract::StepRequest;
use crate::mcp::Session;
use crate::process::{Secret, SharedLog, SpawnRequest};

/// The adapter's working binary name.
pub const BINARY: &str = "copilot";

/// Default per-step timeout when the pipeline does not set one.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(real::DEFAULT_TIMEOUT_SECS);

pub(super) const DISCOVERY: real::Discovery = real::Discovery {
    dir: ".github/agents",
    suffix: ".agent.md",
};

const CREDENTIAL_VARS: [&str; 1] = ["GH_TOKEN"];

fn permission_flags(permissions: &[Permission], worktree: &Path) -> Vec<String> {
    let (write, push) = real::push_boundary(permissions);
    if !write {
        return vec!["--deny-tool=shell(*)".to_owned()];
    }
    let forge = real::FORGE_GRANTS
        .iter()
        .any(|grant| permissions.contains(grant));
    let mut flags = vec![
        "--allow-tool=write".to_owned(),
        "--allow-tool=shell".to_owned(),
        "--add-dir".to_owned(),
        worktree.to_string_lossy().into_owned(),
    ];
    if !push {
        flags.push("--deny-tool=shell(git push)".to_owned());
    }
    if !forge {
        flags.push("--deny-tool=shell(gh:*)".to_owned());
    }
    flags
}

fn argv(role: &Role, worktree: &Path) -> Vec<String> {
    let mut argv = vec![
        BINARY.to_owned(),
        "--acp".to_owned(),
        "--stdio".to_owned(),
        "--experimental".to_owned(),
        "--sandbox".to_owned(),
        "--allow-tool=bureau-io".to_owned(),
    ];
    argv.extend(permission_flags(&role.permissions, worktree));
    argv
}

/// Builds the ACP launch request, retaining scoped native grants and credentials.
/// Stdin is reserved for the official ACP client.
#[must_use]
pub fn spawn_request(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> SpawnRequest {
    let found = real::scoped_credentials(&role.permissions, &real::FORGE_GRANTS, &CREDENTIAL_VARS);
    let (env, secrets) = real::child_env(found, secrets);
    SpawnRequest {
        argv: argv(role, &request.worktree),
        dir: request.worktree.clone(),
        env,
        stdin: Vec::new(),
        timeout: real::timeout(step),
        secrets,
        log,
        cancel: super::cancel_path(request),
    }
}

fn enable_telemetry(env: &mut std::collections::BTreeMap<String, String>, path: &Path) {
    env.insert("COPILOT_OTEL_ENABLED".to_owned(), "true".to_owned());
    env.insert("COPILOT_OTEL_EXPORTER_TYPE".to_owned(), "file".to_owned());
    env.insert(
        "COPILOT_OTEL_FILE_EXPORTER_PATH".to_owned(),
        path.to_string_lossy().into_owned(),
    );
}

async fn read_usage(path: PathBuf) -> Usage {
    let read = tokio::task::spawn_blocking(move || std::fs::read(path)).await;
    match read {
        Ok(Ok(bytes)) => Usage::from_copilot_otel(&bytes),
        _ => Usage::unknown("copilot"),
    }
}

fn merge_usage(execution: &mut Execution, measured: Usage) {
    if execution.usage.cost_usd.is_none() {
        execution.usage = measured;
        return;
    }
    execution.usage.input_tokens = measured.input_tokens;
    execution.usage.output_tokens = measured.output_tokens;
    execution.usage.credits = measured.credits;
}

fn prepare(
    role: &Role,
    step: &StepDef,
    request: &StepRequest,
    timeout: Duration,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> Result<(Session, PathBuf, SpawnRequest), String> {
    let session = Session::create(request).map_err(|error| error.to_string())?;
    let telemetry = session.dir().join("copilot-otel.jsonl");
    let mut built = spawn_request(role, step, request, secrets, log);
    built.timeout = timeout;
    built.env.extend(session.env().clone());
    enable_telemetry(&mut built.env, &telemetry);
    Ok((session, telemetry, built))
}

/// Executes one fresh ACP session and preserves adapter-owned usage.
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
    let mut execution = acp::execute(role, request, &session, built).await;
    merge_usage(&mut execution, read_usage(telemetry).await);
    execution
}
