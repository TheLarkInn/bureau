//! Plugin resolution, durable run snapshots, and temporary activation.
//!
//! A [`Resolver`] accepts only `/plugin:agent` role references. On first
//! activation in a run it resolves target-repository, user-global, then
//! development sources and copies the selected plugin to
//! `<run-dir>/plugins/<plugin>`. Resumes validate and reuse that copy.
//!
//! The content digest is SHA-256 over sorted relative paths, exact file
//! bytes, and normalized file permissions.

pub(crate) mod activation;
pub(crate) mod catalog;
pub(crate) mod error;
pub(crate) mod global;
pub(crate) mod guard;
pub(crate) mod json;
pub(crate) mod paths;
pub(crate) mod reference;
pub(crate) mod resolve;
pub(crate) mod settings;
pub(crate) mod snapshot;
pub(crate) mod tree;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use error::Error;

use reference::AgentReference;
use settings::Settings;
use snapshot::Snapshot;

/// Whether a value has the exact `/plugin:agent` reference shape.
#[must_use]
pub fn is_plugin_reference(value: &str) -> bool {
    AgentReference::parse(value).is_ok()
}

/// Exact identity of one durable plugin snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSource {
    /// Plugin name.
    pub name: String,
    /// Target-repo, user-global, or development source description.
    pub source: String,
    /// Declared semantic version.
    pub version: String,
    /// Content digest of the materialized plugin.
    pub digest: String,
}

/// One active direct agent file installation.
#[derive(Debug)]
pub struct DirectActivation {
    agent_name: String,
    guard: guard::Guard,
}

/// Temporarily materializes pinned direct-agent bytes for both adapters.
///
/// # Errors
/// Rejects invalid paths and propagates exact-activation failures.
pub fn activate_direct(
    agent_path: &str,
    bytes: &[u8],
    worktree: &Path,
) -> Result<DirectActivation, Error> {
    let agent_name = direct_agent_name(agent_path)?;
    let guard = activation::apply_direct(bytes, &agent_name, worktree)?;
    Ok(DirectActivation { agent_name, guard })
}

impl DirectActivation {
    /// Restores every path changed for this activation.
    ///
    /// # Errors
    /// Reports conflicts or restoration failures.
    pub fn restore(mut self) -> Result<(), Error> {
        self.guard.restore()
    }

    /// Adapter discovery name for the pinned agent.
    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }
}

fn direct_agent_name(value: &str) -> Result<String, Error> {
    let path = Path::new(value);
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::InvalidReference(value.to_owned()))?;
    let base = file.strip_suffix(".agent.md").unwrap_or(file);
    Ok(base.strip_suffix(".md").unwrap_or(base).to_owned())
}

/// Resolves and temporarily activates role plugins for one durable run.
#[derive(Debug)]
pub struct Resolver {
    run_dir: PathBuf,
    copilot_home: Option<PathBuf>,
}

impl Resolver {
    /// Creates a resolver for `run_dir` and an optional `COPILOT_HOME`.
    #[must_use]
    pub fn new(run_dir: impl Into<PathBuf>, copilot_home: Option<PathBuf>) -> Self {
        Self {
            run_dir: run_dir.into(),
            copilot_home,
        }
    }

    /// Resolves, snapshots, and activates one `/plugin:agent` reference.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid references, unavailable or unsafe plugin
    /// sources, invalid activation JSON, or filesystem failures.
    pub fn activate(&self, agent_reference: &str, worktree: &Path) -> Result<Activation, Error> {
        let reference = AgentReference::parse(agent_reference)?;
        paths::ensure_outside(&self.run_dir, worktree)?;
        let settings = Settings::read(worktree)?;
        let snapshot = self.snapshot(&reference, worktree, &settings)?;
        let guard = activation::apply(&snapshot, &reference.agent, worktree, &settings)?;
        Ok(Activation {
            source: snapshot.source,
            agent_name: reference.agent,
            guard,
        })
    }

    fn snapshot(
        &self,
        reference: &AgentReference,
        worktree: &Path,
        settings: &Settings,
    ) -> Result<Snapshot, Error> {
        if let Some(snapshot) = snapshot::load(&self.run_dir, &reference.plugin)? {
            return Ok(snapshot);
        }
        let resolved = resolve::find(
            &reference.plugin,
            worktree,
            settings,
            self.copilot_home.as_deref(),
        )?;
        snapshot::create(&self.run_dir, &reference.plugin, &resolved)
    }
}

/// One active worktree-local plugin installation.
///
/// Dropping this value performs best-effort restoration. Prefer [`Self::restore`]
/// so conflicting edits or restoration failures can be escalated.
#[derive(Debug)]
pub struct Activation {
    source: PluginSource,
    agent_name: String,
    guard: guard::Guard,
}

impl Activation {
    /// Metadata for the exact durable plugin snapshot.
    #[must_use]
    pub const fn metadata(&self) -> &PluginSource {
        &self.source
    }

    /// Agent name requested after the plugin prefix.
    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Restores all activation files and returns the selected plugin metadata.
    ///
    /// # Errors
    ///
    /// Returns a conflict after restoring originals when injected bytes
    /// changed, or a restoration error when exact cleanup was not possible.
    pub fn restore(mut self) -> Result<PluginSource, Error> {
        self.guard.restore()?;
        Ok(self.source)
    }
}
