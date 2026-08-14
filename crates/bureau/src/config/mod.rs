//! The config loader (DESIGN.md sections 5–6).
//!
//! Config lives in git, in a repository separate from the code and from
//! the repos being worked on; PR review of that repo is the entire
//! authorization model. Loading accumulates every error found into one
//! `Vec`.

mod files;
mod pipeline;
mod validate;

pub use files::{
    Access, Assignment, ForgeKind, Limits, Named, Permission, Repo, ReposFile, Role, WorkSource,
};
pub use pipeline::{Pipeline, StepDef, StepKind, TERMINALS};
pub use validate::{validate, validate_pipelines};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::ConfigError;

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

impl Config {
    /// Loads a config directory, accumulating every error found.
    ///
    /// Cross-reference validation runs only when parsing is clean; both
    /// phases report all errors in one pass.
    ///
    /// # Errors
    /// Returns every parse, schema, and reference error found.
    pub fn load(dir: &Path) -> Result<Self, Vec<ConfigError>> {
        let mut errors = Vec::new();
        let repos = load_repos(dir, &mut errors);
        let roles = load_named::<Role>(dir, "roles", &mut errors);
        let assignments = load_named::<Assignment>(dir, "assignments", &mut errors);
        let pipelines = load_named::<Pipeline>(dir, "pipelines", &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        let config = Self {
            repos,
            roles,
            assignments,
            pipelines,
        };
        let errors = validate(&config);
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }
}

fn load_repos(dir: &Path, errors: &mut Vec<ConfigError>) -> BTreeMap<String, Repo> {
    let path = dir.join("repos.yaml");
    if !path.exists() {
        errors.push(ConfigError::new(&path, "missing repos.yaml"));
        return BTreeMap::new();
    }
    match load_one::<ReposFile>(&path) {
        Ok(file) => file.repos,
        Err(e) => {
            errors.push(e);
            BTreeMap::new()
        }
    }
}

fn load_named<T>(dir: &Path, sub: &str, errors: &mut Vec<ConfigError>) -> BTreeMap<String, T>
where
    T: DeserializeOwned + Named,
{
    let mut map = BTreeMap::new();
    for path in yaml_paths(&dir.join(sub)) {
        match load_one::<T>(&path) {
            Ok(item) => insert_named(&mut map, item, &path, errors),
            Err(e) => errors.push(e),
        }
    }
    map
}

fn insert_named<T: Named>(
    map: &mut BTreeMap<String, T>,
    item: T,
    path: &Path,
    errors: &mut Vec<ConfigError>,
) {
    let name = item.name().to_owned();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if stem != name {
        errors.push(ConfigError::new(
            path,
            format!("declared name `{name}` does not match file name `{stem}`"),
        ));
    }
    if map.insert(name.clone(), item).is_some() {
        errors.push(ConfigError::new(path, format!("duplicate name `{name}`")));
    }
}

fn yaml_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new(); // a missing subdir means an empty collection
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_yaml(p))
        .collect()
}

fn is_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml" | "yml")
    )
}

fn load_one<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::new(path, &e))?;
    serde_yaml_ng::from_str(&text).map_err(|e| ConfigError::new(path, &e))
}
