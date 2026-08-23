//! Label-rule validation kept separate from assignment validation.

use std::path::Path;

use super::{Config, LabelRule, path_of, push};
use crate::ConfigError;
use crate::forge::ForgeKind;

fn check_text(errors: &mut Vec<ConfigError>, name: &str, rule: &LabelRule, path: &Path) {
    let fields = [
        ("work.source", rule.work.source.as_str()),
        ("work.filter", rule.work.filter.as_str()),
    ];
    for (field, value) in fields {
        if value.trim().is_empty() {
            push(
                errors,
                path.to_path_buf(),
                format!("label rule `{name}`: `{field}` must not be empty"),
            );
        }
    }
}

fn overlap(rule: &LabelRule) -> bool {
    rule.add_labels.iter().any(|add| {
        rule.remove_labels
            .iter()
            .any(|remove| add.trim().eq_ignore_ascii_case(remove.trim()))
    })
}

fn check_labels(errors: &mut Vec<ConfigError>, name: &str, rule: &LabelRule, path: &Path) {
    let all_empty = rule.add_labels.is_empty() && rule.remove_labels.is_empty();
    let has_blank = rule
        .add_labels
        .iter()
        .chain(&rule.remove_labels)
        .any(|label| label.trim().is_empty());
    let checks = [
        (all_empty, "must add or remove at least one label"),
        (has_blank, "labels must not be empty"),
        (overlap(rule), "the same label cannot be added and removed"),
    ];
    for (invalid, message) in checks {
        if invalid {
            push(
                errors,
                path.to_path_buf(),
                format!("label rule `{name}`: {message}"),
            );
        }
    }
}

fn check_limit(errors: &mut Vec<ConfigError>, name: &str, rule: &LabelRule, path: &Path) {
    if rule.limits.max_updates_per_hour == 0 {
        push(
            errors,
            path.to_path_buf(),
            format!("label rule `{name}`: `limits.max_updates_per_hour` must be positive"),
        );
    }
}

fn check_forge(errors: &mut Vec<ConfigError>, name: &str, rule: &LabelRule, path: &Path) {
    if rule.work.forge != ForgeKind::Github {
        push(
            errors,
            path.to_path_buf(),
            format!("label rule `{name}`: `dependencies_closed` currently requires GitHub"),
        );
    }
}

pub(super) fn check(errors: &mut Vec<ConfigError>, config: &Config, name: &str, rule: &LabelRule) {
    let path = path_of("label_rules", name);
    check_text(errors, name, rule, &path);
    check_labels(errors, name, rule, &path);
    check_limit(errors, name, rule, &path);
    check_forge(errors, name, rule, &path);
    if rule.source_repo(&config.repos).is_none() {
        push(
            errors,
            path,
            format!("label rule `{name}`: `work.source` must match a registered repo"),
        );
    }
}

pub(super) fn check_all(errors: &mut Vec<ConfigError>, config: &Config) {
    for (name, rule) in &config.label_rules {
        check(errors, config, name, rule);
    }
}
