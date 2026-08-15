//! The `copilot` adapter: runs the GitHub Copilot CLI as the agent.
//!
//! The agent file a developer invokes locally runs unmodified in
//! automation (DESIGN.md section 6): a `/plugin:agent` reference is
//! copied verbatim from `.ai/plugins/<plugin>/agents/<name>.agent.md`
//! when it resolves locally and otherwise passed through by name — the
//! plugin is expected in the environment (container provisioning,
//! section 10). A direct `.md` path is copied verbatim into
//! `<worktree>/.github/agents/<name>.agent.md`: discovery needs the
//! suffix, so only the file is renamed, never the content.
//!
//! argv is `copilot -p <request-json> --agent <name> --model <model>`;
//! the request JSON also arrives on stdin per the layer-2 contract.
//! The push boundary is mirrored in argv (section 10), and a role
//! without a write grant is denied shell outright — the tool grammar
//! matches commands, not effects, so a read-only shell is not
//! expressible:
//!
//! | permissions                   | flags |
//! |-------------------------------|-------|
//! | `repo:write`, not `repo:push` | `--allow-tool=shell(git:*) --deny-tool='shell(git push)'` |
//! | `repo:push`                   | `--allow-tool=shell(git:*)` |
//! | anything else                 | `--deny-tool='shell(*)'` |
//!
//! Credentials arrive by env convention, gated on the role's grants
//! (section 10): `GH_TOKEN` is a forge credential, forwarded into the
//! child env and added to the scrub list only when the role holds one
//! of [`real::FORGE_GRANTS`]. To record a fixture for the `fake`
//! adapter, run
//! `bureau fake record out.json -- <the argv spawn_request builds>`.

use std::time::Duration;

use super::real;
use crate::config::{Permission, Role, StepDef};
use crate::contract::{StepRequest, StepResult};
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

/// The push-boundary mirror; see the module table. Without a write
/// grant the CLI gets a deny-by-default flag instead of silence.
fn permission_flags(permissions: &[Permission]) -> Vec<String> {
    let (write, push) = real::push_boundary(permissions);
    if !write {
        return vec!["--deny-tool=shell(*)".to_owned()];
    }
    let mut flags = vec!["--allow-tool=shell(git:*)".to_owned()];
    if !push {
        flags.push("--deny-tool=shell(git push)".to_owned());
    }
    flags
}
/// `copilot -p <json> --agent <name> --model <model>` plus the mirror.
fn argv(role: &Role, agent: &str, prompt: &[u8]) -> Vec<String> {
    let mut argv = vec![
        BINARY.to_owned(),
        "-p".to_owned(),
        String::from_utf8_lossy(prompt).into_owned(),
        "--agent".to_owned(),
        agent.to_owned(),
        "--model".to_owned(),
        role.model.clone(),
    ];
    argv.extend(permission_flags(&role.permissions));
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
        argv: argv(role, &agent, &json),
        dir: request.worktree.clone(),
        env,
        stdin: json,
        timeout: real::timeout(step),
        secrets,
        log,
    }
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
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> StepResult {
    let built = spawn_request(role, step, request, secrets, log);
    super::result_from_spawn(&crate::process::spawn(built).await)
}
