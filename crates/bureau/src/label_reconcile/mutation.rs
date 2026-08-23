//! Bounded label mutation and its durable audit events.

use std::sync::Arc;
use std::time::Duration;

use super::{Action, Error, contains};
use crate::config::LabelRule;
use crate::forge::{Dependency, Item, LabelForge};
use crate::state::{LabelRuleAudit, LabelRuleEventKind, Store};

const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(30);

fn dependency_counts(dependencies: &[Dependency]) -> (u32, u32) {
    let total = u32::try_from(dependencies.len()).unwrap_or(u32::MAX);
    let closed = dependencies
        .iter()
        .filter(|dependency| dependency.closed)
        .count();
    (total, u32::try_from(closed).unwrap_or(u32::MAX))
}

fn audit(
    rule: &LabelRule,
    item: &Item,
    dependencies: &[Dependency],
) -> Result<LabelRuleAudit, Error> {
    let add_labels = rule
        .add_labels
        .iter()
        .filter(|label| !contains(&item.labels, label))
        .cloned()
        .collect();
    let remove_labels = rule
        .remove_labels
        .iter()
        .filter(|label| contains(&item.labels, label))
        .cloned()
        .collect();
    let (dependency_count, closed_dependency_count) = dependency_counts(dependencies);
    Ok(LabelRuleAudit {
        attempt_id: crate::identity::random_hex()
            .map_err(|error| Error::identity(rule, item, error))?,
        rule: rule.name.clone(),
        source: rule.source_identity(),
        item: item.external_id.clone(),
        add_labels,
        remove_labels,
        dependency_count,
        closed_dependency_count,
    })
}

fn listed(labels: &[String]) -> String {
    labels
        .iter()
        .map(|label| format!("`{label}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn started_message(audit: &LabelRuleAudit) -> String {
    format!(
        "Work item `{}` unblocked; graduating blocked work item to {} under label rule `{}` ({}/{} dependencies closed); removing {}.",
        audit.item,
        listed(&audit.add_labels),
        audit.rule,
        audit.closed_dependency_count,
        audit.dependency_count,
        listed(&audit.remove_labels),
    )
}

fn terminal_message(audit: &LabelRuleAudit, failure: Option<&str>) -> String {
    failure.map_or_else(
        || {
            format!(
                "Work item `{}` label graduation applied under rule `{}`: added {}; removed {}.",
                audit.item,
                audit.rule,
                listed(&audit.add_labels),
                listed(&audit.remove_labels),
            )
        },
        |detail| {
            format!(
                "Label graduation failed for work item `{}` under rule `{}`: {detail}. The next reconcile pass will retry.",
                audit.item, audit.rule
            )
        },
    )
}

async fn dependencies(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
) -> Result<Vec<Dependency>, Error> {
    let future = forge.blocking_dependencies(&item.external_id);
    tokio::time::timeout(EXTERNAL_TIMEOUT, future)
        .await
        .map_err(|_| Error::timeout(rule, item, "reading blocking dependencies"))?
        .map_err(|error| Error::forge(rule, item, error))
}

fn record(
    state: &Store,
    audit: &LabelRuleAudit,
    kind: LabelRuleEventKind,
    message: &str,
    rule: &LabelRule,
    item: &Item,
) -> Result<(), Error> {
    state
        .record_label_rule_event(audit, kind, message)
        .map_err(|error| Error::state(rule, item, error))
}

fn record_applied(
    state: &Store,
    audit: &LabelRuleAudit,
    rule: &LabelRule,
    item: &Item,
) -> Result<Action, Error> {
    let message = terminal_message(audit, None);
    record(
        state,
        audit,
        LabelRuleEventKind::UpdateApplied,
        &message,
        rule,
        item,
    )?;
    Ok(Action::Applied)
}

fn record_failed(
    state: &Store,
    audit: &LabelRuleAudit,
    rule: &LabelRule,
    item: &Item,
    error: crate::forge::Error,
) -> Result<Action, Error> {
    let message = terminal_message(audit, Some(&error.to_string()));
    record(
        state,
        audit,
        LabelRuleEventKind::UpdateFailed,
        &message,
        rule,
        item,
    )?;
    Err(Error::forge(rule, item, error))
}

fn record_result(
    state: &Store,
    audit: &LabelRuleAudit,
    rule: &LabelRule,
    item: &Item,
    result: Result<(), crate::forge::Error>,
) -> Result<Action, Error> {
    match result {
        Ok(()) => record_applied(state, audit, rule, item),
        Err(error) => record_failed(state, audit, rule, item, error),
    }
}

async fn mutate(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
    audit: &LabelRuleAudit,
) -> Result<Action, Error> {
    let future = forge.update_labels(&item.external_id, &audit.add_labels, &audit.remove_labels);
    let result = tokio::time::timeout(EXTERNAL_TIMEOUT, future)
        .await
        .map_err(|_| crate::forge::Error::Parse("label update timed out".to_owned()))
        .and_then(std::convert::identity);
    record_result(state, audit, rule, item, result)
}

async fn update(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
    audit: &LabelRuleAudit,
) -> Result<Action, Error> {
    let started = started_message(audit);
    let reserved = state
        .begin_label_rule_update(audit, rule.limits.max_updates_per_hour, &started)
        .map_err(|error| Error::state(rule, item, error))?;
    if !reserved {
        return Ok(Action::Limited);
    }
    mutate(rule, item, forge, state, audit).await
}

fn superseded_message(audit: &LabelRuleAudit) -> String {
    format!(
        "Label graduation attempt `{}` for work item `{}` was superseded by the current label rule configuration.",
        audit.attempt_id, audit.item
    )
}

async fn retry_update(
    rule: &LabelRule,
    item: &Item,
    old: &LabelRuleAudit,
    new: &LabelRuleAudit,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
) -> Result<Action, Error> {
    let old_message = superseded_message(old);
    let new_message = started_message(new);
    let reserved = state
        .supersede_label_rule_update(
            old,
            new,
            rule.limits.max_updates_per_hour,
            (&old_message, &new_message),
        )
        .map_err(|error| Error::state(rule, item, error))?;
    if !reserved {
        return Ok(Action::Limited);
    }
    mutate(rule, item, forge, state, new).await
}

pub(super) async fn apply(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
) -> Result<Action, Error> {
    let dependencies = dependencies(rule, item, forge).await?;
    if dependencies.iter().any(|dependency| !dependency.closed) {
        return Ok(Action::Skipped);
    }
    let audit = audit(rule, item, &dependencies)?;
    if audit.add_labels.is_empty() && audit.remove_labels.is_empty() {
        return Ok(Action::Skipped);
    }
    update(rule, item, forge, state, &audit).await
}

pub(super) async fn retry(
    rule: &LabelRule,
    item: &Item,
    old: &LabelRuleAudit,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
) -> Result<Action, Error> {
    let dependencies = dependencies(rule, item, forge).await?;
    if dependencies.iter().any(|dependency| !dependency.closed) {
        return Ok(Action::Skipped);
    }
    let new = audit(rule, item, &dependencies)?;
    if new.add_labels.is_empty() && new.remove_labels.is_empty() {
        record(
            state,
            old,
            LabelRuleEventKind::UpdateAbandoned,
            &superseded_message(old),
            rule,
            item,
        )?;
        return Ok(Action::Skipped);
    }
    retry_update(rule, item, old, &new, forge, state).await
}
