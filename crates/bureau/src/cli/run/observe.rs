use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::{Assignment, Config};
use bureau::forge::{Forge, Item};
use bureau::process::Secret;

use crate::cli::{out, prepare};

#[cfg(test)]
mod tests;

pub(super) struct Prepared {
    pub(super) forge: Arc<dyn Forge>,
    pub(super) item: Item,
    pub(super) credentials: BTreeMap<String, Secret>,
    pub(super) open_prs: usize,
}

async fn open_pr_count(
    config: &Config,
    assignment: &Assignment,
    forge: &dyn Forge,
) -> anyhow::Result<usize> {
    let primary = assignment
        .primary_repo()
        .and_then(|name| config.repos.get(name))
        .context("assignment has no primary repo")?;
    let prs = forge
        .open_prs(&primary.url, &assignment.branch_prefix)
        .await
        .context("observing open pull requests before fresh admission")?;
    Ok(prs.len())
}

async fn approved(
    forge: &dyn Forge,
    assignment: &Assignment,
    item_query: &str,
) -> anyhow::Result<Option<Item>> {
    let Some(item) = prepare::find_item(forge, assignment, item_query).await? else {
        out::error(format_args!(
            "no item `{item_query}` in `{}`",
            assignment.work.source
        ));
        return Ok(None);
    };
    let Some(item) = bureau::reconcile::approved_item(assignment, item) else {
        out::error(format_args!(
            "item `{item_query}` is missing the required approval label"
        ));
        return Ok(None);
    };
    Ok(Some(item))
}

async fn prepare_item(
    config: &Config,
    assignment: &Assignment,
    item_query: &str,
    forge: Arc<dyn Forge>,
    credentials: BTreeMap<String, Secret>,
) -> anyhow::Result<Option<Prepared>> {
    let Some(item) = approved(&*forge, assignment, item_query).await? else {
        return Ok(None);
    };
    let open_prs = open_pr_count(config, assignment, &*forge).await?;
    Ok(Some(Prepared {
        forge,
        item,
        credentials,
        open_prs,
    }))
}

pub(super) async fn prepare_execution(
    config: &Config,
    assignment: &Assignment,
    settings: &bureau::setup::Settings,
    item_query: &str,
) -> anyhow::Result<Option<Prepared>> {
    let Some(credentials) = prepare::resolve_credentials(config, assignment, settings) else {
        return Ok(None);
    };
    let forge = prepare::work_forge(config, assignment, &credentials)?;
    prepare_item(config, assignment, item_query, forge, credentials).await
}
