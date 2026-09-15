//! Offline, snapshot-only factory context. Persist the returned value in the
//! fenced intent before initializing the SDK root. Never call `prepare` to
//! recover an existing intent: `restore` validates only its saved private pins.
//!
//! `plugin_directories` is CREATE-only data. Resume must omit that field,
//! working-directory overrides, and config discovery; no resume wire is built
//! here. Standalone repository agents, skills, commands, instructions, and
//! `.claude` settings are excluded, not claimed as loaded. Hooks and executable
//! MCP contributions are refused, except the exact bundled bureau-io declaration.
//!
//! The parent must map `bureau-io` to the current verified Bureau executable,
//! `["mcp", "serve"]`, `mcp::Session::env()`, and only `get_step_context` /
//! `publish_result`. `bureau_io_plugins` identifies plugin contributions needing
//! that same controlled binding; do not let a plugin fallback resolve `bureau`
//! through PATH or start a duplicate uncontrolled server. Inline agent MCP is
//! refused even for bureau-io because its startup precedes invocation filtering.
//! Pinning does not approve extension execution or grant tools. The parent's
//! prelaunch provider, exact transitive root grants, and OS isolation still apply.

mod agent;
mod audit;
mod files;
mod manifest;
mod mcp;

use std::path::{Path, PathBuf};

use bureau_plugin::pinned::PluginSnapshot;
use bureau_plugin::{Resolver, TreeSnapshot};

use crate::adapters;
use crate::config::Role;
use crate::engine::RunPlan;
use crate::home::{Environment as _, ProcessEnvironment};

pub use crate::adapters::copilot_factory::context_types::{
    CustomAgent, ExpectedCatalog, PinnedContext, PinnedPlugin, ResourceIdentity,
};

fn copilot_home() -> Option<PathBuf> {
    let environment = ProcessEnvironment;
    environment
        .value("COPILOT_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            ["HOME", "USERPROFILE"].into_iter().find_map(|name| {
                environment
                    .value(name)
                    .map(|value| PathBuf::from(value).join(".copilot"))
            })
        })
}

fn plugin_name(role: &Role) -> Option<String> {
    bureau_plugin::copilot_agent_name(&role.agent)
        .and_then(|name| name.split_once(':').map(|(plugin, _)| plugin.to_owned()))
}

fn role_snapshot(
    plan: &RunPlan,
    role: &Role,
    worktree: &Path,
    resolver: &Resolver,
) -> Result<Option<PluginSnapshot>, String> {
    let Some(name) = plugin_name(role) else {
        return Ok(None);
    };
    let snapshot = plan.plugin_sources.get(&name).map_or_else(
        || resolver.snapshot_only(&role.agent, worktree),
        |expected| resolver.require_snapshot(expected),
    );
    snapshot.map(Some).map_err(|error| error.to_string())
}

fn direct_agent(plan: &RunPlan, role: &Role) -> Result<Option<CustomAgent>, String> {
    if bureau_plugin::is_plugin_reference(&role.agent) {
        return Ok(None);
    }
    let bytes = plan.direct_agents.get(&role.name).ok_or_else(|| {
        format!(
            "direct agent for role `{}` is absent from the immutable run plan",
            role.name
        )
    })?;
    agent::parse(bytes, &adapters::expected_agent(role)).map(Some)
}

fn pin(snapshot: PluginSnapshot, root: &Path, worktree: &Path) -> Result<PinnedPlugin, String> {
    let destination = root.join("plugins").join(&snapshot.source.name);
    files::outside(&destination, worktree)?;
    let tree = TreeSnapshot::pin(&snapshot.directory, &destination, &snapshot.source.digest)
        .map_err(|error| error.to_string())?;
    Ok(PinnedPlugin {
        source: snapshot.source,
        directory: tree.directory().to_path_buf(),
    })
}

fn add_direct(pins: &PinnedContext, catalog: &mut ExpectedCatalog) -> Result<(), String> {
    let Some(agent) = &pins.custom_agent else {
        return Ok(());
    };
    agent::validate(agent)?;
    let prior = catalog
        .agents
        .insert(agent.name.clone(), ResourceIdentity::Inline);
    if agent.name != pins.selected_agent || prior.is_some() {
        return Err(
            "direct agent identity conflicts with the saved selection or catalog".to_owned(),
        );
    }
    catalog.agent_skills.insert(
        agent.name.clone(),
        agent.skills.iter().flatten().cloned().collect(),
    );
    Ok(())
}

fn skill_dependencies(catalog: &ExpectedCatalog) -> Result<(), String> {
    if let Some(skill) = catalog
        .agent_skills
        .values()
        .flatten()
        .find(|skill| !catalog.skills.contains_key(*skill))
    {
        return Err(format!(
            "custom-agent skill `{skill}` is not provided by the approved pinned plugins"
        ));
    }
    Ok(())
}

fn catalog(pins: &PinnedContext) -> Result<ExpectedCatalog, String> {
    let mut catalog = ExpectedCatalog::default();
    catalog.mcp_servers.insert(
        "bureau-io".to_owned(),
        ["get_step_context".to_owned(), "publish_result".to_owned()].into(),
    );
    for plugin in &pins.plugins {
        audit::plugin(plugin, &mut catalog)?;
    }
    add_direct(pins, &mut catalog)?;
    skill_dependencies(&catalog)?;
    if !catalog.agents.contains_key(&pins.selected_agent) {
        return Err(format!(
            "selected agent `{}` is absent from pinned context",
            pins.selected_agent
        ));
    }
    Ok(catalog)
}

fn check_plugin_path(pins: &PinnedContext, plugin: &PinnedPlugin) -> Result<(), String> {
    if !bureau_plugin::is_plugin_reference(&format!("/{}:snapshot", plugin.source.name)) {
        return Err("saved plugin name is invalid".to_owned());
    }
    let expected = pins.private_root.join("plugins").join(&plugin.source.name);
    if plugin.directory != expected || files::canonical(&plugin.directory)? != expected {
        return Err("plugin pin directory differs from its saved private location".to_owned());
    }
    files::outside(&plugin.directory, &pins.worktree)
}

fn check_plugin_paths(pins: &PinnedContext) -> Result<(), String> {
    let mut names = std::collections::BTreeSet::new();
    for plugin in &pins.plugins {
        check_plugin_path(pins, plugin)?;
        if !names.insert(&plugin.source.name) {
            return Err("durable context lists the same plugin more than once".to_owned());
        }
    }
    Ok(())
}

/// Verifies saved identities and whole trees without resolving or replacing sources.
///
/// # Errors
/// Rejects missing, changed, unsafe, or differently identified context material.
pub(super) fn restore(pins: &PinnedContext) -> Result<(), String> {
    if files::canonical(&pins.worktree)? != pins.worktree
        || files::canonical(&pins.private_root)? != pins.private_root
    {
        return Err("saved context roots must remain existing canonical absolute paths".to_owned());
    }
    files::outside(&pins.private_root, &pins.worktree)?;
    check_plugin_paths(pins)?;
    if catalog(pins)? != pins.expected_catalog {
        return Err("pinned context catalog differs from the durable factory intent".to_owned());
    }
    Ok(())
}

fn snapshots(
    plan: &RunPlan,
    role: &Role,
    worktree: &Path,
    resolver: &Resolver,
) -> Result<Vec<PluginSnapshot>, String> {
    let selected = role_snapshot(plan, role, worktree, resolver)?;
    let name = plugin_name(role);
    let retained = name
        .as_deref()
        .filter(|name| plan.plugin_sources.contains_key(*name));
    let mut sources = resolver
        .repository_snapshots(worktree, retained)
        .map_err(|error| error.to_string())?;
    sources.retain(|source| Some(source.source.name.as_str()) != name.as_deref());
    sources.extend(selected);
    sources.sort_by(|left, right| left.source.name.cmp(&right.source.name));
    Ok(sources)
}

fn prepared(
    plan: &RunPlan,
    role: &Role,
    worktree: &Path,
    private_root: &Path,
    resolver: &Resolver,
) -> Result<PinnedContext, String> {
    files::repository(worktree)?;
    let custom_agent = direct_agent(plan, role)?;
    let snapshots = snapshots(plan, role, worktree, resolver)?;
    let private_root = files::private_root(private_root, worktree)?;
    let plugins = snapshots
        .into_iter()
        .map(|snapshot| pin(snapshot, &private_root, worktree))
        .collect::<Result<_, _>>()?;
    let mut pins = PinnedContext {
        worktree: files::canonical(worktree)?,
        private_root,
        plugins,
        selected_agent: adapters::expected_agent(role),
        custom_agent,
        expected_catalog: ExpectedCatalog::default(),
    };
    pins.expected_catalog = catalog(&pins)?;
    restore(&pins)?;
    Ok(pins)
}

/// Prepares approved local context without activation, installation, or SDK calls.
///
/// # Errors
/// Rejects unavailable material, unsafe contributions, and incompatible identities.
pub(super) fn prepare(
    plan: &RunPlan,
    role: &Role,
    worktree: &Path,
    run_dir: &Path,
    private_root: &Path,
) -> Result<PinnedContext, String> {
    if role.adapter != adapters::AdapterKind::Copilot {
        return Err("local factory context requires a Copilot role".to_owned());
    }
    files::outside(run_dir, worktree)?;
    prepared(
        plan,
        role,
        worktree,
        private_root,
        &Resolver::new(run_dir, copilot_home()),
    )
}

#[cfg(test)]
mod tests;
