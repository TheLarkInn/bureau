//! Shared pieces for the real (`copilot`, `claude`) adapters.
//!
//! Deliberately small: agent-file resolution and verbatim
//! materialization (DESIGN.md section 6), the push-boundary permission
//! mirror (section 10), permission-gated credential forwarding, and
//! step timeouts.
//!
//! Section 10 checks permissions before spawn: each grant maps to a
//! credential that layer 0 does or does not inject. An adapter reads a
//! credential variable from the daemon's environment only when the
//! role holds a grant mapped to it:
//!
//! | credential variables                           | grants |
//! |------------------------------------------------|--------|
//! | `GH_TOKEN` (a forge credential)                | [`FORGE_GRANTS`] |
//! | `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN` (model credentials) | [`MODEL_GRANTS`] |

use crate::config::{Permission, StepDef};
use crate::process::Secret;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default per-step timeout, in seconds, when the pipeline sets none.
pub const DEFAULT_TIMEOUT_SECS: u64 = 1800;

/// The `bureau-io` MCP server definition both real adapters hand to
/// their CLI: a stdio server running `bureau mcp serve`.
const MCP_CONFIG: &str =
    r#"{"mcpServers":{"bureau-io":{"type":"stdio","command":"bureau","args":["mcp","serve"]}}}"#;

/// Writes the `bureau-io` MCP server config to `path`, so the agent CLI
/// can start the server. Shared by both real adapters (issue #17).
///
/// # Errors
/// Propagates filesystem failures.
pub fn write_mcp_config(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, MCP_CONFIG)
}

/// Non-secret runtime variables agent CLIs and plugin subprocesses need.
const RUNTIME_VARS: [&str; 5] = [
    "PATH",
    "HOME",
    "COPILOT_HOME",
    "CLAUDE_CONFIG_DIR",
    "XDG_CONFIG_HOME",
];

/// Where an adapter discovers agent files inside a worktree.
pub struct Discovery {
    /// Discovery directory under the worktree (e.g. `.github/agents`).
    pub dir: &'static str,
    /// File suffix discovery requires (e.g. `.agent.md`).
    pub suffix: &'static str,
}

/// Splits a `/plugin:agent` reference; anything else is a path.
fn plugin_parts(agent: &str) -> Option<(&str, &str)> {
    if !bureau_plugin::is_plugin_reference(agent) {
        return None;
    }
    agent.strip_prefix('/')?.split_once(':')
}

/// The agent name a path implies: file name minus discovery suffixes.
fn agent_name(source: &Path) -> String {
    let file = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("agent");
    let base = file.strip_suffix(".agent.md").unwrap_or(file);
    base.strip_suffix(".md").unwrap_or(base).to_owned()
}

/// Copies the agent file into the worktree's discovery dir, verbatim.
///
/// Only the file name changes to fit discovery, never the content.
/// `None` on any I/O failure — see [`resolve_agent`].
fn copy_agent(
    source: &Path,
    worktree: &Path,
    discovery: &Discovery,
    name: &str,
) -> Option<PathBuf> {
    let content = std::fs::read(source).ok()?;
    let dest = worktree
        .join(discovery.dir)
        .join(format!("{name}{}", discovery.suffix));
    std::fs::create_dir_all(dest.parent()?).ok()?;
    std::fs::write(&dest, content).ok()?;
    Some(dest)
}

/// Resolves a role's agent reference to the `--agent` value
/// (DESIGN.md section 6).
///
/// The engine activates a pinned `/plugin:agent` before this call, so a
/// plugin reference only contributes its agent name. A direct `.md` path
/// is materialized verbatim for adapter discovery.
pub fn resolve_agent(agent: &str, worktree: &Path, discovery: &Discovery) -> String {
    if let Some((_, name)) = plugin_parts(agent) {
        return name.to_owned();
    }
    if !agent.to_ascii_lowercase().ends_with(".md") {
        return agent.to_owned();
    }
    let source = Path::new(agent);
    let name = agent_name(source);
    let _copied = copy_agent(source, worktree, discovery, &name);
    name
}

/// The push boundary a role's permissions imply, as `(write, push)`.
///
/// `repo:push` implies `repo:write` — a push is a write that lands.
/// The adapters mirror this line in argv (DESIGN.md section 10):
/// `write` allows the git shell, `!push` still denies `git push`, and
/// no write grant at all denies the shell outright (deny by default).
pub fn push_boundary(permissions: &[Permission]) -> (bool, bool) {
    let has = |p: Permission| permissions.contains(&p);
    let push = has(Permission::RepoPush);
    (push || has(Permission::RepoWrite), push)
}

/// Remote grants that map to a forge credential (DESIGN.md section 10);
/// local worktree reads and writes need no token.
pub const FORGE_GRANTS: [Permission; 8] = [
    Permission::RepoPush,
    Permission::IssuesRead,
    Permission::IssuesWrite,
    Permission::PrRead,
    Permission::PrWrite,
    Permission::PrReview,
    Permission::PrMerge,
    Permission::RunsRead,
];

/// The grant that maps to a model credential (DESIGN.md section 10).
pub const MODEL_GRANTS: [Permission; 1] = [Permission::ModelInvoke];

/// Reads one variable from the daemon's environment; a non-Unicode
/// value counts as missing, matching `std::env::var` semantics. This
/// is the module's one process-environment boundary.
fn env_value(name: &str) -> Option<String> {
    use crate::home::{Environment, ProcessEnvironment};
    ProcessEnvironment
        .value(name)
        .and_then(|value| value.into_string().ok())
}

/// Reads credential variables from the daemon's environment; missing or
/// empty values are skipped. Callers gate through [`scoped_credentials`]
/// so a role without the mapped grant receives nothing.
pub fn daemon_credentials(names: &[&str]) -> Vec<(String, String)> {
    let found = |name: &str| {
        env_value(name)
            .filter(|value| !value.is_empty())
            .map(|value| (name.to_owned(), value))
    };
    names.iter().copied().filter_map(found).collect()
}

/// The non-secret runtime environment (PATH, HOME, …) every step
/// subprocess needs, read from the daemon. Contains no credentials.
#[must_use]
pub fn runtime_env() -> BTreeMap<String, String> {
    daemon_credentials(&RUNTIME_VARS).into_iter().collect()
}

/// Reads `names` from the daemon's environment when `permissions` hold
/// any of `grants`, and reads nothing otherwise — the section-10 check
/// before spawn, applied at the one place credentials cross over.
pub fn scoped_credentials(
    permissions: &[Permission],
    grants: &[Permission],
    names: &[&str],
) -> Vec<(String, String)> {
    if grants.iter().any(|grant| permissions.contains(grant)) {
        return daemon_credentials(names);
    }
    Vec::new()
}

/// The step's spawn timeout.
pub fn timeout(step: &StepDef) -> Duration {
    Duration::from_secs(step.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS))
}

/// Builds the complete child env and scrub list from found credentials.
///
/// Pure: the found values arrive as parameters so tests need no
/// `set_var`. Every forwarded value joins the scrub list so it cannot
/// leak through captured output or the run log.
pub fn child_env(
    found: Vec<(String, String)>,
    mut secrets: Vec<Secret>,
) -> (BTreeMap<String, String>, Vec<Secret>) {
    let mut env: BTreeMap<String, String> = daemon_credentials(&RUNTIME_VARS).into_iter().collect();
    env.extend(found.iter().cloned());
    secrets.extend(found.into_iter().map(|(_, value)| Secret::new(value)));
    (env, secrets)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        FORGE_GRANTS, MODEL_GRANTS, RUNTIME_VARS, agent_name, child_env, push_boundary,
        scoped_credentials,
    };
    use crate::config::Permission;
    use crate::process::Secret;

    #[test]
    fn child_env_forwards_and_scrubs_found_credentials() {
        let found = vec![("GH_TOKEN".to_owned(), "tok".to_owned())];
        let (env, secrets) = child_env(found, vec![Secret::new("prior")]);
        let seen = (
            env.get("GH_TOKEN").map(String::as_str),
            secrets.contains(&Secret::new("tok")),
            secrets.contains(&Secret::new("prior")),
        );
        assert_eq!(seen, (Some("tok"), true, true));
    }

    #[test]
    fn child_env_without_credentials_carries_no_secrets() {
        let (env, secrets) = child_env(Vec::new(), Vec::new());
        let expected: std::collections::BTreeSet<_> = RUNTIME_VARS
            .into_iter()
            .filter(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
            .map(str::to_owned)
            .collect();
        assert_eq!(
            (env.keys().cloned().collect(), secrets.is_empty()),
            (expected, true)
        );
    }

    #[test]
    fn push_boundary_reflects_the_grant() {
        let cases = [
            (vec![Permission::RepoWrite], (true, false)),
            (vec![Permission::RepoPush], (true, true)),
            (vec![Permission::RepoRead], (false, false)),
        ];
        for (permissions, expected) in cases {
            assert_eq!(push_boundary(&permissions), expected);
        }
    }

    /// The section-10 mapping as `(permission, forge, model)`: which
    /// grant set forwards a credential for each permission.
    const MAPPING: [(Permission, bool, bool); 11] = [
        (Permission::RepoRead, false, false),
        (Permission::RepoWrite, false, false),
        (Permission::RepoPush, true, false),
        (Permission::PrRead, true, false),
        (Permission::PrWrite, true, false),
        (Permission::PrReview, true, false),
        (Permission::PrMerge, true, false),
        (Permission::IssuesRead, true, false),
        (Permission::IssuesWrite, true, false),
        (Permission::RunsRead, true, false),
        (Permission::ModelInvoke, false, true),
    ];

    /// `PATH` stands in for a credential variable because a test
    /// process always has it set and non-empty.
    #[test]
    fn scoped_credentials_follow_the_permission_mapping() {
        let found = |p: Permission, grants: &[Permission]| {
            scoped_credentials(&[p], grants, &["PATH"]).len()
        };
        for (permission, forge, model) in MAPPING {
            let seen = (
                found(permission, &FORGE_GRANTS),
                found(permission, &MODEL_GRANTS),
            );
            assert_eq!(
                seen,
                (usize::from(forge), usize::from(model)),
                "{permission:?}"
            );
        }
        assert!(scoped_credentials(&[], &FORGE_GRANTS, &["PATH"]).is_empty());
    }

    #[test]
    fn agent_name_strips_discovery_suffixes() {
        let names = (
            agent_name(Path::new("dir/reviewer.md")),
            agent_name(Path::new("reviewer.agent.md")),
        );
        assert_eq!(names, ("reviewer".to_owned(), "reviewer".to_owned()));
    }
}
