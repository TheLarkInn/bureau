//! Cross-reference validation. Every check accumulates into one
//! `Vec<ConfigError>` — `bureau validate` never bails on the first `?`.

use std::path::{Path, PathBuf};

use super::Config;
use super::StepKind;
use super::files::{Assignment, ForgeKind, Limits, Repo, Role};
use crate::ConfigError;
use crate::contract::Trust;

pub(super) fn path_of(kind: &str, name: &str) -> PathBuf {
    PathBuf::from(format!("{kind}/{name}.yaml"))
}

pub(super) fn push(errors: &mut Vec<ConfigError>, path: PathBuf, message: String) {
    errors.push(ConfigError { path, message });
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
    check_approval_label(errors, name, a, &path);
    check_ado_approval(errors, config, name, a, &path);
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
    let integers = [
        ("max_concurrent", limits.max_concurrent.map(u64::from)),
        ("max_runs_per_hour", limits.max_runs_per_hour.map(u64::from)),
        ("max_runs_per_day", limits.max_runs_per_day.map(u64::from)),
        ("max_open_prs", limits.max_open_prs.map(u64::from)),
        ("max_run_hours", limits.max_run_hours),
    ];
    check_positive_integers(errors, name, path, integers);
    if limits
        .max_cost_per_day_usd
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        limit_error(errors, name, path, "max_cost_per_day_usd");
    }
}

fn check_positive_integers(
    errors: &mut Vec<ConfigError>,
    name: &str,
    path: &Path,
    checks: [(&str, Option<u64>); 5],
) {
    for (field, value) in checks {
        if value == Some(0) {
            limit_error(errors, name, path, field);
        }
    }
}

fn limit_error(errors: &mut Vec<ConfigError>, name: &str, path: &Path, field: &str) {
    push(
        errors,
        path.to_path_buf(),
        format!("assignment `{name}`: remove `{field}` or set it to a positive value"),
    );
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

fn check_approval_label(
    errors: &mut Vec<ConfigError>,
    name: &str,
    assignment: &Assignment,
    path: &Path,
) {
    let blank = assignment
        .work
        .approval_label
        .as_deref()
        .is_some_and(|label| label.trim().is_empty());
    if blank {
        push(
            errors,
            path.to_path_buf(),
            format!("assignment `{name}`: remove `work.approval_label` or name the approval label"),
        );
    }
}

fn check_ado_approval(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    assignment: &Assignment,
    path: &Path,
) {
    if assignment.work.forge != ForgeKind::Ado || assignment.work.approval_label.is_some() {
        return;
    }
    let Some(pipeline) = config.pipelines.get(&assignment.pipeline) else {
        return;
    };
    for step in &pipeline.steps {
        check_ado_step(errors, config, name, step, path);
    }
}

fn check_ado_step(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    assignment: &str,
    step: &super::StepDef,
    path: &Path,
) {
    if step.kind != StepKind::Agent {
        return;
    }
    let role_name = step.role.as_deref().unwrap_or_default();
    let minimum = step
        .trust
        .or_else(|| config.roles.get(role_name).map(|role| role.min_trust));
    if minimum.is_some_and(|trust| trust > Trust::Untrusted) {
        push(
            errors,
            path.to_path_buf(),
            approval_error(assignment, &step.name, role_name),
        );
    }
}

fn approval_error(assignment: &str, step: &str, role: &str) -> String {
    format!(
        "assignment `{assignment}`: add `work.approval_label` so approved ADO items can reach step `{step}`; ADO items are untrusted until they carry that label, but role `{role}` requires higher trust"
    )
}
