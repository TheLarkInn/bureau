//! `bureau` — a local agent work runner.
//!
//! A single-binary daemon that reconciles desired state (config in git)
//! against observed state (the forge) by running agent-driven pipelines
//! against git worktrees. `DESIGN.md` at the repository root is the
//! authoritative spec; the naming law there is enforced in review.
//!
//! Start here: [`process`] is the layer-0 contract everything spawns
//! through; [`engine`] drives pipelines; [`reconcile`] is the loop.

pub mod adapters;
pub mod config;
pub mod contract;
pub mod credential;
pub mod doctor;
pub mod engine;
pub mod forge;
pub mod git;
mod identity;
pub mod mcp;
pub mod process;
pub mod reconcile;
pub mod repair;
pub mod runlog;
pub mod state;
pub mod supervise;
pub mod watch;
pub use bureau_lifecycle::{home, maintenance, setup};

use std::path::{Path, PathBuf};

/// One configuration problem, tied to the file that caused it.
///
/// Loading and validation accumulate these into a `Vec` so
/// `bureau validate` reports every error in one pass.
#[derive(Debug, serde::Serialize, thiserror::Error)]
#[error("{}: {message}", .path.display())]
pub struct ConfigError {
    /// The file (or synthetic `dir/name` path) the error belongs to.
    #[serde(serialize_with = "display_path")]
    pub path: PathBuf,
    /// What is wrong.
    pub message: String,
}

impl ConfigError {
    /// Creates an error for `path` with a displayable `message`.
    #[must_use]
    pub fn new(path: &Path, message: impl std::fmt::Display) -> Self {
        Self {
            path: path.to_path_buf(),
            message: message.to_string(),
        }
    }
}

fn display_path<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.collect_str(&path.display())
}
