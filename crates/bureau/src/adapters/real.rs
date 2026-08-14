//! Shared pieces for the real (`copilot`, `claude`) adapters.
//!
//! Deliberately small: agent-file resolution and verbatim
//! materialization (DESIGN.md section 6), the push-boundary permission
//! mirror (section 10), credential forwarding by env convention, and
//! step timeouts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{Permission, StepDef};
use crate::process::Secret;

/// Default per-step timeout, in seconds, when the pipeline sets none.
pub const DEFAULT_TIMEOUT_SECS: u64 = 1800;

/// The step's spawn timeout.
pub fn timeout(step: &StepDef) -> Duration {
    Duration::from_secs(step.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS))
}

/// Where an adapter discovers agent files inside a worktree.
pub struct Discovery {
    /// Discovery directory under the worktree (e.g. `.github/agents`).
    pub dir: &'static str,
    /// File suffix discovery requires (e.g. `.agent.md`).
    pub suffix: &'static str,
}

/// Resolves a role's agent reference to the `--agent` value
/// (DESIGN.md section 6).
///
/// A `/plugin:agent` reference resolves from
/// `.ai/plugins/<plugin>/agents/<name>.agent.md` under the daemon's
/// current directory; when that file does not resolve, the name alone
/// is passed through and the plugin is expected to be installed in the
/// environment (container provisioning, section 10). A direct `.md`
/// path always materializes. Copies are verbatim and best effort: on
/// any I/O failure nothing is copied, the CLI resolves the agent name
/// itself, and its error surfaces as a step `Failure`.
pub fn resolve_agent(agent: &str, worktree: &Path, discovery: &Discovery) -> String {
    if let Some((plugin, name)) = plugin_parts(agent) {
        let source = plugin_agent_path(plugin, name);
        let _copied = copy_agent(&source, worktree, discovery, name);
        return name.to_owned();
    }
    let source = Path::new(agent);
    let name = agent_name(source);
    let _copied = copy_agent(source, worktree, discovery, &name);
    name
}

/// Splits a `/plugin:agent` reference; anything else is a path.
fn plugin_parts(agent: &str) -> Option<(&str, &str)> {
    agent.strip_prefix('/')?.split_once(':')
}

/// The local plugin cache location of an agent file.
fn plugin_agent_path(plugin: &str, name: &str) -> PathBuf {
    Path::new(".ai")
        .join("plugins")
        .join(plugin)
        .join("agents")
        .join(format!("{name}.agent.md"))
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

/// The push boundary a role's permissions imply, as `(write, push)`.
///
/// `repo:push` implies `repo:write` — a push is a write that lands.
/// Only this credential line is mirrored in argv (DESIGN.md section
/// 10); absent grants keep the CLI's default behavior, so no grants
/// mean no flags.
pub fn push_boundary(permissions: &[Permission]) -> (bool, bool) {
    let has = |p: Permission| permissions.contains(&p);
    let push = has(Permission::RepoPush);
    (push || has(Permission::RepoWrite), push)
}

/// Reads credential variables from the daemon's environment.
///
/// Reading is safe — only `set_var` is `unsafe` on edition 2024. A
/// missing or empty variable is not forwarded; the engine resolves
/// repo credentials itself, so that is never an error here.
pub fn daemon_credentials(names: &[&str]) -> Vec<(String, String)> {
    let found = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .map(|value| (name.to_owned(), value))
    };
    names.iter().copied().filter_map(found).collect()
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
    let env = found.iter().cloned().collect();
    secrets.extend(found.into_iter().map(|(_, value)| Secret::new(value)));
    (env, secrets)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{agent_name, child_env, push_boundary};
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
    fn child_env_without_credentials_leaves_env_empty() {
        let (env, secrets) = child_env(Vec::new(), Vec::new());
        assert_eq!((env.is_empty(), secrets.is_empty()), (true, true));
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

    #[test]
    fn agent_name_strips_discovery_suffixes() {
        let names = (
            agent_name(Path::new("dir/reviewer.md")),
            agent_name(Path::new("reviewer.agent.md")),
        );
        assert_eq!(names, ("reviewer".to_owned(), "reviewer".to_owned()));
    }
}
