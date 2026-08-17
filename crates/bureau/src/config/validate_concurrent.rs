//! Cross-step rules for static concurrent evidence groups.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::files::Permission;
use super::pipeline::{Pipeline, StepDef, StepKind};
use super::{Config, push, step_err};
use crate::ConfigError;

const EVIDENCE_PERMISSIONS: [Permission; 5] = [
    Permission::RepoRead,
    Permission::IssuesRead,
    Permission::PrRead,
    Permission::RunsRead,
    Permission::ModelInvoke,
];

fn check_group_limit(errors: &mut Vec<ConfigError>, name: &str, group: &StepDef, path: &Path) {
    if group
        .max_concurrent
        .is_some_and(|limit| usize::try_from(limit).map_or(true, |limit| limit > group.steps.len()))
    {
        step_err(
            errors,
            path,
            name,
            &group.name,
            "`max_concurrent` cannot exceed the number of listed steps",
        );
    }
    if !group.inputs_from.is_empty() {
        step_err(
            errors,
            path,
            name,
            &group.name,
            "`inputs_from` belongs on concurrent member steps",
        );
    }
}

fn member_error(
    errors: &mut Vec<ConfigError>,
    path: &Path,
    name: &str,
    group: &StepDef,
    member: &str,
    detail: &str,
) {
    let message = format!(
        "pipeline `{name}` concurrent group `{}` member `{member}` {detail}",
        group.name
    );
    push(errors, path.to_path_buf(), message);
}

fn member_problem(
    pipeline: &Pipeline,
    group: &StepDef,
    membership: &mut BTreeMap<String, String>,
    unique: &mut BTreeSet<String>,
    member: &String,
) -> Option<String> {
    if !unique.insert(member.clone()) {
        return Some("is listed more than once".to_owned());
    }
    if !pipeline.steps.iter().any(|step| step.name == *member) {
        return Some("does not exist".to_owned());
    }
    membership
        .insert(member.clone(), group.name.clone())
        .map(|first| format!("already belongs to concurrent group `{first}`"))
}

fn add_members(
    errors: &mut Vec<ConfigError>,
    name: &str,
    pipeline: &Pipeline,
    group: &StepDef,
    path: &Path,
    membership: &mut BTreeMap<String, String>,
) {
    let mut unique = BTreeSet::new();
    for member in &group.steps {
        if let Some(detail) = member_problem(pipeline, group, membership, &mut unique, member) {
            member_error(errors, path, name, group, member, &detail);
        }
    }
}

fn membership(
    errors: &mut Vec<ConfigError>,
    name: &str,
    pipeline: &Pipeline,
    path: &Path,
) -> BTreeMap<String, String> {
    let mut membership = BTreeMap::new();
    for group in pipeline
        .steps
        .iter()
        .filter(|step| step.kind == StepKind::Concurrent)
    {
        check_group_limit(errors, name, group, path);
        add_members(errors, name, pipeline, group, path, &mut membership);
    }
    membership
}

fn check_member_kind(
    errors: &mut Vec<ConfigError>,
    name: &str,
    pipeline: &Pipeline,
    step: &StepDef,
    group: &str,
    path: &Path,
) {
    let entry = pipeline.steps.first().map(|entry| entry.name.as_str());
    if entry == Some(step.name.as_str()) {
        step_err(
            errors,
            path,
            name,
            &step.name,
            "a concurrent member cannot be the entry",
        );
    }
    if !matches!(step.kind, StepKind::Deterministic | StepKind::Agent) {
        step_err(
            errors,
            path,
            name,
            &step.name,
            &format!("member of `{group}` must be deterministic or agent"),
        );
    }
}

fn check_member_edges(errors: &mut Vec<ConfigError>, name: &str, step: &StepDef, path: &Path) {
    if step.edge_targets().next().is_some() {
        step_err(
            errors,
            path,
            name,
            &step.name,
            "a concurrent member cannot have outcome edges; the group owns routing",
        );
    }
}

fn check_sibling_inputs(
    errors: &mut Vec<ConfigError>,
    name: &str,
    pipeline: &Pipeline,
    step: &StepDef,
    group: &str,
    path: &Path,
) {
    let Some(group) = pipeline.steps.iter().find(|step| step.name == group) else {
        return;
    };
    for input in &step.inputs_from {
        if group.steps.contains(input) {
            step_err(
                errors,
                path,
                name,
                &step.name,
                &format!("cannot consume concurrent sibling `{input}`"),
            );
        }
    }
}

fn check_permissions(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    step: &StepDef,
    path: &Path,
) {
    let Some(role) = step.role.as_deref().and_then(|role| config.roles.get(role)) else {
        return;
    };
    if let Some(permission) = role
        .permissions
        .iter()
        .find(|permission| !EVIDENCE_PERMISSIONS.contains(permission))
    {
        step_err(
            errors,
            path,
            name,
            &step.name,
            &format!("concurrent evidence role cannot hold `{permission}`"),
        );
    }
}

fn check_member(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    pipeline: &Pipeline,
    member: &str,
    group: &str,
    path: &Path,
) {
    let Some(step) = pipeline.steps.iter().find(|step| step.name == member) else {
        return;
    };
    check_member_kind(errors, name, pipeline, step, group, path);
    check_member_edges(errors, name, step, path);
    check_sibling_inputs(errors, name, pipeline, step, group, path);
    check_permissions(errors, config, name, step, path);
}

fn check_edge_targets(
    errors: &mut Vec<ConfigError>,
    name: &str,
    pipeline: &Pipeline,
    membership: &BTreeMap<String, String>,
    path: &Path,
) {
    for step in &pipeline.steps {
        for target in step.edge_targets() {
            if let Some(group) = membership.get(target) {
                step_err(
                    errors,
                    path,
                    name,
                    &step.name,
                    &format!("edge targets concurrent member `{target}` from group `{group}`"),
                );
            }
        }
    }
}

pub(super) fn check(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    pipeline: &Pipeline,
    path: &Path,
) {
    let membership = membership(errors, name, pipeline, path);
    for (member, group) in &membership {
        check_member(errors, config, name, pipeline, member, group, path);
    }
    check_edge_targets(errors, name, pipeline, &membership, path);
}
