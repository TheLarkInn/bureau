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
pub mod engine;
pub mod forge;
pub mod git;
pub mod process;
pub mod reconcile;
pub mod runlog;
pub mod state;

use std::fmt;
use std::path::Path;

/// One configuration problem, tied to the file that caused it.
///
/// Loading and validation accumulate these into a `Vec` so
/// `bureau validate` reports every error in one pass.
#[derive(Debug, thiserror::Error)]
#[error("{path}: {message}")]
pub struct ConfigError {
    /// The file (or synthetic `dir/name` path) the error belongs to.
    pub path: String,
    /// What is wrong.
    pub message: String,
}

impl ConfigError {
    /// Creates an error for `path` with a displayable `message`.
    #[must_use]
    pub fn new(path: &Path, message: impl fmt::Display) -> Self {
        Self {
            path: path.display().to_string(),
            message: message.to_string(),
        }
    }
}
