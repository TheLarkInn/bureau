//! The loaded configuration's in-memory shape (DESIGN.md sections
//! 5–6). Kept free of loading and validation logic so `mod`, `validate`,
//! and `validate_pipeline` can all depend on it without a sibling cycle.

use std::collections::BTreeMap;

use super::files::{Assignment, Repo, Role};
use super::pipeline::Pipeline;

/// The loaded runner configuration: the repo registry plus every role
/// and assignment.
#[derive(Debug, Clone)]
pub struct Config {
    /// Every repo the runner may touch, by short name.
    pub repos: BTreeMap<String, Repo>,
    /// Role definitions by name.
    pub roles: BTreeMap<String, Role>,
    /// Standing arrangements by name.
    pub assignments: BTreeMap<String, Assignment>,
    /// Step state machines by name.
    pub pipelines: BTreeMap<String, Pipeline>,
}
