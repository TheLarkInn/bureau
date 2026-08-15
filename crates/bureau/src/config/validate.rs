//! Cross-reference validation. Every check accumulates into one
//! `Vec<ConfigError>` — `bureau validate` never bails on the first `?`.

use std::path::{Path, PathBuf};

use super::Config;
use super::files::{Assignment, Limits, Repo, Role};
use crate::ConfigError;

pub(super) fn path_of(kind: &str, name: &str) -> PathBuf {
    PathBuf::from(format!("{kind}/{name}.yaml"))
}

pub(super) fn push(errors: &mut Vec<ConfigError>, path: PathBuf, message: String) {
    errors.push(ConfigError { path, message });
}

/// Pipeline validation (DESIGN.md layer 4).
///
/// Covers per-kind required fields, edge resolution against steps and
/// terminals, complete decision coverage, `inputs_from` references, role
/// references, fixture rules, and the assignment-to-pipeline reference.
#[must_use]
pub fn validate_pipelines(config: &Config) -> Vec<ConfigError> {
    super::validate_pipeline::validate_pipelines(config)
}
fn check_repo(errors: &mut Vec<ConfigError>, name: &str, repo: &Repo) {
    let path = PathBuf::from("repos.yaml");
    if repo.url.trim().is_empty() {
        push(
            errors,
            path.clone(),
            format!("repo `{name}`: `url` must not be empty"),
        );
    }
    if repo.credential.trim().is_empty() {
        push(
            errors,
            path,
            format!("repo `{name}`: `credential` must not be empty"),
        );
    }
}
fn check_role(errors: &mut Vec<ConfigError>, name: &str, role: &Role) {
    let path = path_of("roles", name);
    if role.concurrency == 0 {
        push(
            errors,
            path.clone(),
            format!("role `{name}`: `concurrency` must be at least 1"),
        );
    }
    if !(role.agent.starts_with('/') || role.agent.to_ascii_lowercase().ends_with(".md")) {
        push(
            errors,
            path,
            format!(
                "role `{name}`: `agent` must be a plugin invocation (`/plugin:agent`) or a path to an agent .md"
            ),
        );
    }
}
fn check_repo_refs(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    a: &Assignment,
    path: &Path,
) {
    if a.repos.is_empty() {
        push(
            errors,
            path.to_path_buf(),
            format!("assignment `{name}` lists no repos"),
        );
    }
    for repo in &a.repos {
        if !config.repos.contains_key(repo) {
            push(
                errors,
                path.to_path_buf(),
                format!("assignment `{name}` references unknown repo `{repo}`"),
            );
        }
    }
}
fn check_primary_access(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    a: &Assignment,
    path: &Path,
) {
    let Some(primary) = a.primary_repo() else {
        return;
    };
    let Some(repo) = config.repos.get(primary) else {
        return; // already reported by check_repo_refs
    };
    if !repo.access.allows_push() {
        push(
            errors,
            path.to_path_buf(),
            format!(
                "assignment `{name}`: primary repo `{primary}` has access `read`; the branch cannot land"
            ),
        );
    }
}
fn check_limits(errors: &mut Vec<ConfigError>, name: &str, limits: &Limits, path: &Path) {
    let checks = [
        ("max_concurrent", f64::from(limits.max_concurrent)),
        ("max_runs_per_hour", f64::from(limits.max_runs_per_hour)),
        ("max_runs_per_day", f64::from(limits.max_runs_per_day)),
        ("max_open_prs", f64::from(limits.max_open_prs)),
        ("max_cost_per_day_usd", limits.max_cost_per_day_usd),
    ];
    for (field, value) in checks {
        if value <= 0.0 {
            push(
                errors,
                path.to_path_buf(),
                format!("assignment `{name}`: `{field}` must be positive"),
            );
        }
    }
}
fn check_text(errors: &mut Vec<ConfigError>, name: &str, a: &Assignment, path: &Path) {
    let fields = [
        ("pipeline", a.pipeline.as_str()),
        ("verify", a.verify.as_str()),
        ("branch_prefix", a.branch_prefix.as_str()),
        ("work.source", a.work.source.as_str()),
        ("work.filter", a.work.filter.as_str()),
    ];
    for (field, value) in fields {
        if value.trim().is_empty() {
            push(
                errors,
                path.to_path_buf(),
                format!("assignment `{name}`: `{field}` must not be empty"),
            );
        }
    }
}
fn check_assignment(errors: &mut Vec<ConfigError>, config: &Config, name: &str, a: &Assignment) {
    let path = path_of("assignments", name);
    check_repo_refs(errors, config, name, a, &path);
    check_primary_access(errors, config, name, a, &path);
    if !config.roles.contains_key(&a.role) {
        push(
            errors,
            path.clone(),
            format!("assignment `{name}` references unknown role `{}`", a.role),
        );
    }
    check_limits(errors, name, &a.limits, &path);
    check_text(errors, name, a, &path);
}
/// All validation rules. Returns every problem found in one pass.
#[must_use]
pub fn validate(config: &Config) -> Vec<ConfigError> {
    let mut errors = Vec::new();
    for (name, repo) in &config.repos {
        check_repo(&mut errors, name, repo);
    }
    for (name, role) in &config.roles {
        check_role(&mut errors, name, role);
    }
    for (name, assignment) in &config.assignments {
        check_assignment(&mut errors, config, name, assignment);
    }
    errors.extend(validate_pipelines(config));
    errors
}
