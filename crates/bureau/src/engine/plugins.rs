//! Durable plugin pinning before execution and per-step activation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::RunPlan;
use crate::config::{Role, StepKind};
use crate::home::Environment;
use crate::runlog::PluginSource;
use bureau_plugin::{DirectActivation, Resolver};

pub(super) enum PrepareError {
    Failure(String),
    Blocked(String),
}

/// The plugin home: `COPILOT_HOME`, else the platform home's
/// `.copilot`. The process environment is read only here, through the
/// lifecycle crate's boundary.
fn copilot_home() -> Option<PathBuf> {
    let environment = crate::home::ProcessEnvironment;
    environment
        .value("COPILOT_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .value("HOME")
                .map(|home| PathBuf::from(home).join(".copilot"))
        })
        .or_else(|| {
            environment
                .value("USERPROFILE")
                .map(|home| PathBuf::from(home).join(".copilot"))
        })
}

fn resolver(run_dir: &Path) -> Resolver {
    Resolver::new(run_dir, copilot_home())
}

fn insert_source(
    sources: &mut BTreeMap<String, PluginSource>,
    source: PluginSource,
) -> Result<(), String> {
    if sources
        .get(&source.name)
        .is_some_and(|existing| existing != &source)
    {
        return Err(format!(
            "plugin `{}` resolved to conflicting immutable snapshots",
            source.name
        ));
    }
    sources.insert(source.name.clone(), source);
    Ok(())
}

fn references(plan: &RunPlan) -> BTreeSet<String> {
    plan.pipeline
        .steps
        .iter()
        .filter(|step| step.kind == StepKind::Agent)
        .filter_map(|step| step.role.as_deref())
        .filter_map(|name| plan.roles.get(name))
        .filter(|role| bureau_plugin::is_plugin_reference(&role.agent))
        .map(|role| role.agent.clone())
        .collect()
}

pub(super) fn prepare(
    plan: &mut RunPlan,
    run_dir: &Path,
    worktree: &Path,
) -> Result<(), PrepareError> {
    let resolver = resolver(run_dir);
    let mut sources = BTreeMap::new();
    for reference in references(plan) {
        let activation = resolver.activate(&reference, worktree).map_err(|error| {
            PrepareError::Failure(format!("activating `{reference}` failed: {error}"))
        })?;
        let source = activation.restore().map_err(|error| {
            PrepareError::Blocked(format!("restoring `{reference}` failed: {error}"))
        })?;
        insert_source(&mut sources, source).map_err(PrepareError::Failure)?;
    }
    plan.plugin_sources = sources;
    Ok(())
}

pub(super) enum ActiveAgent {
    Plugin(bureau_plugin::Activation),
    Direct(DirectActivation),
}

impl ActiveAgent {
    pub(super) fn direct_agent_name(&self) -> Option<&str> {
        match self {
            Self::Plugin(_) => None,
            Self::Direct(active) => Some(active.agent_name()),
        }
    }

    fn restore(self) -> Result<(), bureau_plugin::Error> {
        match self {
            Self::Plugin(active) => active.restore().map(|_| ()),
            Self::Direct(active) => active.restore(),
        }
    }
}

fn verify(
    plan: &RunPlan,
    activation: bureau_plugin::Activation,
) -> Result<bureau_plugin::Activation, String> {
    let source = activation.metadata();
    if plan.plugin_sources.get(&source.name) == Some(source) {
        return Ok(activation);
    }
    let message = format!(
        "plugin `{}` does not match the immutable run snapshot",
        source.name
    );
    match activation.restore() {
        Ok(_) => Err(message),
        Err(error) => Err(format!("{message}; restoration also failed: {error}")),
    }
}

fn activate_plugin(
    plan: &RunPlan,
    role: &Role,
    run_dir: &Path,
    worktree: &Path,
) -> Result<ActiveAgent, String> {
    let activation = resolver(run_dir)
        .activate(&role.agent, worktree)
        .map_err(|error| format!("activating `{}` failed: {error}", role.agent))?;
    verify(plan, activation).map(ActiveAgent::Plugin)
}

pub(super) fn activate(
    plan: &RunPlan,
    role: &Role,
    run_dir: &Path,
    worktree: &Path,
) -> Result<Option<ActiveAgent>, String> {
    if bureau_plugin::is_plugin_reference(&role.agent) {
        return activate_plugin(plan, role, run_dir, worktree).map(Some);
    }
    if role.adapter == crate::adapters::AdapterKind::Fake {
        return Ok(None);
    }
    let bytes = plan
        .direct_agents
        .get(&role.name)
        .ok_or_else(|| format!("direct agent for role `{}` was not pinned", role.name))?;
    bureau_plugin::activate_direct(&role.agent, bytes, worktree, run_dir)
        .map(ActiveAgent::Direct)
        .map(Some)
        .map_err(|error| format!("activating `{}` failed: {error}", role.agent))
}

pub(super) fn restore(activation: Option<ActiveAgent>) -> Result<(), String> {
    activation.map_or(Ok(()), |active| {
        active
            .restore()
            .map_err(|error| format!("restoring plugin activation failed: {error}"))
    })
}
