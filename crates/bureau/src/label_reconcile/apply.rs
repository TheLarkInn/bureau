//! Claim and revalidation for one label-rule candidate.

use std::sync::Arc;
use std::time::Duration;

use super::{Action, Error, forge_name, lease_item_id, lease_scope, mutation};
use crate::config::LabelRule;
use crate::forge::{Item, LabelForge};
use crate::state::{LeaseOwner, Store};

const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(30);
const LEASE_TTL: Duration = Duration::from_secs(120);

fn owner(rule: &LabelRule, item: &Item, state: Arc<Store>) -> Result<LeaseOwner, Error> {
    let external_id = lease_item_id(&item.external_id);
    let run_id = format!("label-rule:{}:{external_id}", rule.name);
    LeaseOwner::new(
        state,
        lease_scope(),
        forge_name(rule.work.forge),
        &external_id,
        &run_id,
    )
    .map_err(|error| Error::state(rule, item, error))
}

async fn current(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
) -> Result<Item, Error> {
    let future = forge.item(&item.external_id);
    tokio::time::timeout(EXTERNAL_TIMEOUT, future)
        .await
        .map_err(|_| Error::timeout(rule, item, "rechecking the work item"))?
        .map_err(|error| Error::forge(rule, item, error))
}

async fn evaluate(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
) -> Result<Action, Error> {
    let current = current(rule, item, forge).await?;
    mutation::apply(rule, &current, forge, state).await
}

fn settled(outcome: Result<Action, Error>, released: Result<(), Error>) -> Result<Action, Error> {
    match (outcome, released) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(action), Ok(())) => Ok(action),
    }
}

async fn apply_claimed(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
    state: &Store,
    owner: &LeaseOwner,
) -> Result<Action, Error> {
    let outcome = evaluate(rule, item, forge, state).await;
    let released = owner
        .release()
        .map_err(|error| Error::state(rule, item, error));
    settled(outcome, released)
}

pub(super) async fn item(
    rule: &LabelRule,
    item: &Item,
    forge: &Arc<dyn LabelForge>,
    state: Arc<Store>,
) -> Result<Action, Error> {
    let owner = owner(rule, item, state.clone())?;
    if !owner
        .claim(LEASE_TTL)
        .map_err(|error| Error::state(rule, item, error))?
    {
        return Ok(Action::Skipped);
    }
    apply_claimed(rule, item, forge, &state, &owner).await
}
