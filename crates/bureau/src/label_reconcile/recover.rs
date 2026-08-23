use std::sync::Arc;
use std::time::Duration;

use super::{Action, Error, Recovery, contains, forge_name, lease_item_id, lease_scope, mutation};
use crate::config::LabelRule;
use crate::forge::{Item, LabelForge};
use crate::state::{LabelRuleAudit, LabelRuleEventKind, LeaseOwner, Store};

const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(30);
const LEASE_TTL: Duration = Duration::from_secs(120);

fn applied(audit: &LabelRuleAudit, item: &Item) -> bool {
    audit
        .add_labels
        .iter()
        .all(|label| contains(&item.labels, label))
        && audit
            .remove_labels
            .iter()
            .all(|label| !contains(&item.labels, label))
}

fn message(audit: &LabelRuleAudit, was_applied: bool) -> String {
    if was_applied {
        return format!(
            "Recovered interrupted label graduation for work item `{}` under rule `{}`; current labels confirm the update was applied.",
            audit.item, audit.rule
        );
    }
    format!(
        "Abandoned label graduation for missing work item `{}` under rule `{}`; no further mutation will be attempted.",
        audit.item, audit.rule
    )
}

fn state_error(rule: &LabelRule, item: &str, source: crate::state::Error) -> Error {
    Error::State {
        rule: rule.name.clone(),
        item: item.to_owned(),
        source,
    }
}

fn owner(rule: &LabelRule, audit: &LabelRuleAudit, state: Arc<Store>) -> Result<LeaseOwner, Error> {
    let external_id = lease_item_id(&audit.item);
    let run_id = format!("label-rule:{}:{external_id}", rule.name);
    LeaseOwner::new(
        state,
        lease_scope(),
        forge_name(rule.work.forge),
        &external_id,
        &run_id,
    )
    .map_err(|source| state_error(rule, &audit.item, source))
}

async fn current(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    forge: &Arc<dyn LabelForge>,
) -> Result<Item, Error> {
    let future = forge.item(&audit.item);
    tokio::time::timeout(EXTERNAL_TIMEOUT, future)
        .await
        .map_err(|_| Error::Timeout {
            rule: rule.name.clone(),
            item: audit.item.clone(),
            operation: "recovering an interrupted label update",
        })?
        .map_err(|source| Error::Forge {
            rule: rule.name.clone(),
            item: audit.item.clone(),
            source,
        })
}

fn record(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    kind: LabelRuleEventKind,
    message: &str,
    state: &Store,
) -> Result<(), Error> {
    state
        .record_label_rule_event(audit, kind, message)
        .map_err(|source| state_error(rule, &audit.item, source))
}

fn record_recovered(rule: &LabelRule, audit: &LabelRuleAudit, state: &Store) -> Result<(), Error> {
    record(
        rule,
        audit,
        LabelRuleEventKind::UpdateApplied,
        &message(audit, true),
        state,
    )
}

const fn abandoned(error: &Error) -> bool {
    matches!(
        error,
        Error::Forge {
            source: crate::forge::Error::Api { status: 410, .. },
            ..
        }
    )
}

fn record_abandoned(rule: &LabelRule, audit: &LabelRuleAudit, state: &Store) -> Result<(), Error> {
    record(
        rule,
        audit,
        LabelRuleEventKind::UpdateAbandoned,
        &message(audit, false),
        state,
    )
}

fn record_source_change(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    state: &Store,
) -> Result<(), Error> {
    let message = format!(
        "Abandoned label graduation for work item `{}` because rule `{}` moved from source `{}` to `{}`.",
        audit.item,
        audit.rule,
        audit.source,
        rule.source_identity()
    );
    record(
        rule,
        audit,
        LabelRuleEventKind::UpdateAbandoned,
        &message,
        state,
    )
}

async fn recover_current(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
) -> Result<Option<bool>, Error> {
    if applied(audit, item) {
        record_recovered(rule, audit, state)?;
        return Ok(Some(true));
    }
    match mutation::retry(rule, item, audit, forge, state).await? {
        Action::Applied => Ok(Some(true)),
        Action::Skipped | Action::Limited => Ok(None),
    }
}

fn inspect_error(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    state: &Store,
    error: Error,
) -> Result<Option<bool>, Error> {
    if !abandoned(&error) {
        return Err(error);
    }
    record_abandoned(rule, audit, state)?;
    Ok(None)
}

async fn inspect(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
) -> Result<Option<bool>, Error> {
    if audit.source != rule.source_identity() {
        record_source_change(rule, audit, state)?;
        return Ok(None);
    }
    let item = match current(rule, audit, forge).await {
        Ok(item) => item,
        Err(error) => return inspect_error(rule, audit, state, error),
    };
    recover_current(rule, audit, &item, forge, state).await
}

fn confirmed_pending(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    owner: &LeaseOwner,
    state: &Store,
) -> Result<bool, Error> {
    let pending = state
        .label_rule_update_pending(&audit.attempt_id)
        .map_err(|source| state_error(rule, &audit.item, source));
    match pending {
        Ok(true) => Ok(true),
        Ok(false) => {
            owner
                .release()
                .map_err(|source| state_error(rule, &audit.item, source))?;
            Ok(false)
        }
        Err(error) => match owner.release() {
            Ok(()) => Err(error),
            Err(source) => Err(state_error(rule, &audit.item, source)),
        },
    }
}

async fn attempt(
    rule: &LabelRule,
    audit: &LabelRuleAudit,
    forge: &Arc<dyn LabelForge>,
    state: Arc<Store>,
) -> Result<Option<bool>, Error> {
    let owner = owner(rule, audit, state.clone())?;
    if !owner
        .claim(LEASE_TTL)
        .map_err(|source| state_error(rule, &audit.item, source))?
    {
        return Ok(None);
    }
    if !confirmed_pending(rule, audit, &owner, &state)? {
        return Ok(None);
    }
    let recovered = inspect(rule, audit, forge, &state).await;
    let released = owner
        .release()
        .map_err(|source| state_error(rule, &audit.item, source));
    match (recovered, released) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(result), Ok(())) => Ok(result),
    }
}

fn record_outcome(outcome: &mut Recovery, result: Result<Option<bool>, Error>) -> bool {
    match result {
        Ok(Some(true)) => outcome.applied += 1,
        Ok(Some(false) | None) => {}
        Err(error) => {
            let limited = error.is_rate_limited();
            outcome.errors.push(error);
            return limited;
        }
    }
    false
}

async fn recover_all(
    rule: &LabelRule,
    forge: &Arc<dyn LabelForge>,
    state: Arc<Store>,
    pending: &[LabelRuleAudit],
) -> Recovery {
    let mut outcome = Recovery {
        applied: 0,
        errors: Vec::new(),
    };
    for audit in pending {
        let result = attempt(rule, audit, forge, state.clone()).await;
        if record_outcome(&mut outcome, result) {
            break;
        }
    }
    outcome
}

pub(super) async fn rule(
    rule: &LabelRule,
    forge: &Arc<dyn LabelForge>,
    state: Arc<Store>,
) -> Recovery {
    let pending = state
        .pending_label_rule_updates(&rule.name)
        .map_err(|source| state_error(rule, "audit recovery", source));
    match pending {
        Ok(pending) => recover_all(rule, forge, state, &pending).await,
        Err(error) => Recovery {
            applied: 0,
            errors: vec![error],
        },
    }
}
