//! Deterministic, bounded reconciliation of forge-owned labels.

mod apply;
mod mutation;
mod recover;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{Config, ForgeKind, LabelRule};
use crate::forge::{Item, LabelForge};
use crate::state::Store;

const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum Action {
    Applied,
    Skipped,
    Limited,
}

pub(super) fn contains(labels: &[String], wanted: &str) -> bool {
    labels
        .iter()
        .any(|label| label.eq_ignore_ascii_case(wanted))
}

/// One label-rule reconcile failure with its work-item context.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A configured rule has no available forge client.
    #[error("label rule `{rule}` has no available forge client")]
    MissingForge {
        /// Rule that could not run.
        rule: String,
    },
    /// A forge observation or mutation failed.
    #[error("label rule `{rule}` for work item `{item}`: {source}")]
    Forge {
        /// Rule being evaluated.
        rule: String,
        /// Work item being evaluated, or `query` before items are known.
        item: String,
        /// Forge failure.
        #[source]
        source: crate::forge::Error,
    },
    /// A durable claim or audit operation failed.
    #[error("label rule `{rule}` for work item `{item}`: {source}")]
    State {
        /// Rule being evaluated.
        rule: String,
        /// Work item being evaluated.
        item: String,
        /// State failure.
        #[source]
        source: crate::state::Error,
    },
    /// A bounded external operation reached its deadline.
    #[error("label rule `{rule}` for work item `{item}` timed out while {operation}")]
    Timeout {
        /// Rule being evaluated.
        rule: String,
        /// Work item being evaluated.
        item: String,
        /// Operation that reached its deadline.
        operation: &'static str,
    },
    /// Generating an audit-attempt identity failed.
    #[error(
        "label rule `{rule}` for work item `{item}` could not create an attempt identity: {source}"
    )]
    Identity {
        /// Rule being evaluated.
        rule: String,
        /// Work item being evaluated.
        item: String,
        /// Operating-system random source failure.
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    fn forge(rule: &LabelRule, item: &Item, source: crate::forge::Error) -> Self {
        Self::Forge {
            rule: rule.name.clone(),
            item: item.external_id.clone(),
            source,
        }
    }

    fn state(rule: &LabelRule, item: &Item, source: crate::state::Error) -> Self {
        Self::State {
            rule: rule.name.clone(),
            item: item.external_id.clone(),
            source,
        }
    }

    fn timeout(rule: &LabelRule, item: &Item, operation: &'static str) -> Self {
        Self::Timeout {
            rule: rule.name.clone(),
            item: item.external_id.clone(),
            operation,
        }
    }

    fn identity(rule: &LabelRule, item: &Item, source: std::io::Error) -> Self {
        Self::Identity {
            rule: rule.name.clone(),
            item: item.external_id.clone(),
            source,
        }
    }

    pub(crate) const fn is_rate_limited(&self) -> bool {
        matches!(
            self,
            Self::Forge {
                source: crate::forge::Error::RateLimited { .. },
                ..
            }
        )
    }
}

pub(super) struct Recovery {
    pub(super) applied: usize,
    pub(super) errors: Vec<Error>,
}

/// Activity and independent failures from one label-rule pass.
#[derive(Debug, Default)]
pub struct Pass {
    /// Successfully applied label updates.
    pub applied: usize,
    /// Rule or item failures; later rules still run.
    pub errors: Vec<Error>,
    /// Whether the forge asked this credential to stop making requests.
    pub rate_limited: bool,
}

pub(crate) const fn forge_name(forge: ForgeKind) -> &'static str {
    match forge {
        ForgeKind::Ado => "ado",
        ForgeKind::Github => "github",
    }
}

pub(crate) const fn lease_scope() -> &'static str {
    "label-reconcile"
}

pub(crate) fn lease_item_id(item_id: &str) -> String {
    item_id.to_ascii_lowercase()
}

async fn candidates(rule: &LabelRule, forge: &Arc<dyn LabelForge>) -> Result<Vec<Item>, Error> {
    let future = forge.query(&rule.work.source, &rule.work.filter);
    let result = tokio::time::timeout(EXTERNAL_TIMEOUT, future)
        .await
        .map_err(|_| Error::Timeout {
            rule: rule.name.clone(),
            item: "query".to_owned(),
            operation: "querying candidates",
        })?;
    result.map_err(|source| Error::Forge {
        rule: rule.name.clone(),
        item: "query".to_owned(),
        source,
    })
}

fn record_action(pass: &mut Pass, result: Result<Action, Error>) -> bool {
    match result {
        Ok(Action::Applied) => pass.applied += 1,
        Ok(Action::Skipped) => {}
        Ok(Action::Limited) => return true,
        Err(error) => {
            let limited = error.is_rate_limited();
            pass.errors.push(error);
            return limited;
        }
    }
    false
}

async fn apply_items(
    rule: &LabelRule,
    forge: &Arc<dyn LabelForge>,
    state: Arc<Store>,
    items: &[Item],
) -> Pass {
    let mut pass = Pass::default();
    for item in items {
        let result = apply::item(rule, item, forge, state.clone()).await;
        if record_action(&mut pass, result) {
            break;
        }
    }
    pass
}

fn failed(error: Error, applied: usize) -> Pass {
    Pass {
        applied,
        errors: vec![error],
        rate_limited: false,
    }
}

async fn after_recovery(
    rule: &LabelRule,
    forge: &Arc<dyn LabelForge>,
    state: Arc<Store>,
    recovered: usize,
) -> Pass {
    let items = match candidates(rule, forge).await {
        Ok(items) => items,
        Err(error) => return failed(error, recovered),
    };
    let mut pass = apply_items(rule, forge, state, &items).await;
    pass.applied += recovered;
    pass
}

async fn reconcile_rule(rule: &LabelRule, forge: &Arc<dyn LabelForge>, state: Arc<Store>) -> Pass {
    let recovery = recover::rule(rule, forge, state.clone()).await;
    if recovery.errors.iter().any(Error::is_rate_limited) {
        return Pass {
            applied: recovery.applied,
            errors: recovery.errors,
            rate_limited: true,
        };
    }
    let mut pass = after_recovery(rule, forge, state, recovery.applied).await;
    pass.errors.extend(recovery.errors);
    pass
}

/// Evaluates every configured label rule once.
pub async fn reconcile(
    config: &Config,
    state: Arc<Store>,
    forges: &BTreeMap<String, Arc<dyn LabelForge>>,
) -> Pass {
    let mut pass = Pass::default();
    for rule in config.label_rules.values() {
        let Some(forge) = forges.get(&rule.name) else {
            pass.errors.push(Error::MissingForge {
                rule: rule.name.clone(),
            });
            continue;
        };
        let current = reconcile_rule(rule, forge, state.clone()).await;
        let limited = current.errors.iter().any(Error::is_rate_limited);
        pass.applied += current.applied;
        pass.errors.extend(current.errors);
        if limited {
            pass.rate_limited = true;
            break;
        }
    }
    pass
}
