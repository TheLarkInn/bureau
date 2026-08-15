//! Pipeline validation (DESIGN.md layer 4): cross-step references,
//! edge resolution, fixture rules, data-flow order, and reachability.
//! Per-step field checks live on `StepDef::field_errors`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::Config;
use super::files::{Assignment, Role};
use super::pipeline::{Pipeline, StepDef, StepKind, TERMINALS};
use super::validate::{path_of, push};
use crate::ConfigError;
use crate::adapters::AdapterKind;

fn step_err(errors: &mut Vec<ConfigError>, path: &Path, name: &str, step: &str, detail: &str) {
    let message = format!("pipeline `{name}` step `{step}`: {detail}");
    push(errors, path.to_path_buf(), message);
}
fn visit<'a>(current: &'a str, pipeline: &'a Pipeline, seen: &mut BTreeSet<&'a str>) {
    if !seen.insert(current) {
        return;
    }
    let Some(step) = pipeline.steps.iter().find(|s| s.name == current) else {
        return; // a terminal or an unknown target (reported by check_edge)
    };
    for target in step.edge_targets() {
        visit(target, pipeline, seen);
    }
}
fn check_reachable(errors: &mut Vec<ConfigError>, name: &str, pipeline: &Pipeline, path: &Path) {
    let Some(entry) = pipeline.steps.first() else {
        return; // an empty pipeline is already reported
    };
    let mut seen = BTreeSet::new();
    visit(&entry.name, pipeline, &mut seen);
    for step in &pipeline.steps {
        if !seen.contains(step.name.as_str()) {
            let detail = format!("unreachable from `{}`", entry.name);
            step_err(errors, path, name, &step.name, &detail);
        }
    }
}
fn check_edge(
    errors: &mut Vec<ConfigError>,
    name: &str,
    step: &StepDef,
    field: &str,
    target: &str,
    order: &BTreeMap<&str, usize>,
    path: &Path,
) {
    if order.contains_key(target) {
        return;
    }
    let mut err = |detail: &str| step_err(errors, path, name, &step.name, detail);
    if target == "join" {
        err(&format!(
            "edge `{field}`: join is reserved for fan-out and is not supported in v0"
        ));
    } else if !TERMINALS.contains(&target) {
        err(&format!("edge `{field}` targets unknown step `{target}`"));
    }
}
fn check_named_edges(
    errors: &mut Vec<ConfigError>,
    name: &str,
    step: &StepDef,
    order: &BTreeMap<&str, usize>,
    path: &Path,
) {
    let edges = [
        ("next", step.next.as_deref()),
        ("on_failure", step.on_failure.as_deref()),
        ("on_blocked", step.on_blocked.as_deref()),
        ("on_no_work", step.on_no_work.as_deref()),
    ];
    for (field, target) in edges {
        if let Some(target) = target {
            check_edge(errors, name, step, field, target, order, path);
        }
    }
}
fn check_decision_edges(
    errors: &mut Vec<ConfigError>,
    name: &str,
    step: &StepDef,
    order: &BTreeMap<&str, usize>,
    path: &Path,
) {
    if step.kind != StepKind::Decision {
        return;
    }
    for (outcome, target) in &step.on {
        check_edge(errors, name, step, outcome, target, order, path);
    }
}
fn check_inputs_from(
    errors: &mut Vec<ConfigError>,
    name: &str,
    index: usize,
    step: &StepDef,
    order: &BTreeMap<&str, usize>,
    path: &Path,
) {
    let mut err = |detail: &str| step_err(errors, path, name, &step.name, detail);
    for input in &step.inputs_from {
        match order.get(input.as_str()) {
            Some(&i) if i < index => {}
            Some(_) => err(&format!(
                "`inputs_from` entry `{input}` is not an earlier step"
            )),
            None => err(&format!("`inputs_from` names unknown step `{input}`")),
        }
    }
}
fn check_fixture(
    errors: &mut Vec<ConfigError>,
    name: &str,
    step: &StepDef,
    role: &Role,
    path: &Path,
) {
    let Some(fixture) = step.fixture.as_deref() else {
        return;
    };
    let mut err = |detail: &str| step_err(errors, path, name, &step.name, detail);
    if role.adapter != AdapterKind::Fake {
        err("`fixture` requires a role with the `fake` adapter");
    }
    if !Path::new(fixture).is_absolute() {
        err("`fixture` must be an absolute path");
    }
}
fn check_agent(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    step: &StepDef,
    path: &Path,
) {
    let Some(role_name) = step.role.as_deref() else {
        return; // a missing `role` is already reported by field_errors
    };
    let Some(role) = config.roles.get(role_name) else {
        step_err(
            errors,
            path,
            name,
            &step.name,
            &format!("references unknown role `{role_name}`"),
        );
        return;
    };
    check_fixture(errors, name, step, role, path);
}
fn check_over(
    errors: &mut Vec<ConfigError>,
    name: &str,
    index: usize,
    step: &StepDef,
    order: &BTreeMap<&str, usize>,
    path: &Path,
) {
    let Some(over) = step.over.as_deref() else {
        return; // a missing `over` is already reported by field_errors
    };
    let mut err = |detail: &str| step_err(errors, path, name, &step.name, detail);
    match order.get(over) {
        Some(&i) if i < index => {}
        Some(_) => err(&format!("`over` step `{over}` is not an earlier step")),
        None => err(&format!("`over` names unknown step `{over}`")),
    }
}
fn check_references(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    index: usize,
    step: &StepDef,
    order: &BTreeMap<&str, usize>,
    path: &Path,
) {
    match step.kind {
        StepKind::Agent => check_agent(errors, config, name, step, path),
        StepKind::Decision => check_over(errors, name, index, step, order, path),
        StepKind::Deterministic => {}
    }
}
fn check_step(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    index: usize,
    step: &StepDef,
    order: &BTreeMap<&str, usize>,
    path: &Path,
) {
    for detail in step.field_errors() {
        step_err(errors, path, name, &step.name, &detail);
    }
    check_references(errors, config, name, index, step, order, path);
    check_named_edges(errors, name, step, order, path);
    check_decision_edges(errors, name, step, order, path);
    check_inputs_from(errors, name, index, step, order, path);
}
fn step_order(pipeline: &Pipeline) -> BTreeMap<&str, usize> {
    pipeline
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect()
}
fn check_step_names(errors: &mut Vec<ConfigError>, name: &str, pipeline: &Pipeline, path: &Path) {
    let mut err = |detail: &str| {
        push(
            errors,
            path.to_path_buf(),
            format!("pipeline `{name}` {detail}"),
        );
    };
    if pipeline.steps.is_empty() {
        err("has no steps");
    }
    let mut seen = BTreeSet::new();
    for step in &pipeline.steps {
        if !seen.insert(step.name.as_str()) {
            err(&format!("has duplicate step `{}`", step.name));
        }
    }
}
fn check_assignment_pipeline(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    a: &Assignment,
) {
    let pipeline = a.pipeline.trim();
    if !pipeline.is_empty() && !config.pipelines.contains_key(pipeline) {
        push(
            errors,
            path_of("assignments", name),
            format!("assignment `{name}` references unknown pipeline `{pipeline}`"),
        );
    }
}
fn check_pipeline(errors: &mut Vec<ConfigError>, config: &Config, name: &str, pipeline: &Pipeline) {
    let path = path_of("pipelines", name);
    check_step_names(errors, name, pipeline, &path);
    let order = step_order(pipeline);
    for (index, step) in pipeline.steps.iter().enumerate() {
        check_step(errors, config, name, index, step, &order, &path);
    }
    check_reachable(errors, name, pipeline, &path);
}
/// All pipeline rules, plus the assignment-to-pipeline reference.
#[must_use]
pub fn validate_pipelines(config: &Config) -> Vec<ConfigError> {
    let mut errors = Vec::new();
    for (name, pipeline) in &config.pipelines {
        check_pipeline(&mut errors, config, name, pipeline);
    }
    for (name, assignment) in &config.assignments {
        check_assignment_pipeline(&mut errors, config, name, assignment);
    }
    errors
}
