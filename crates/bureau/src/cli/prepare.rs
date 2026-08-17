//! Pre-spawn preparation for `run`/`retry`: credentials, the work forge
//! client, and the item lookup. Everything here happens before anything
//! spawns (DESIGN.md section 13).

use crate::cli::out;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::Context as _;

use bureau::config::{Assignment, Config, ForgeKind, Repo};
use bureau::forge::ado::AdoForge;
use bureau::forge::github::GitHubForge;
use bureau::forge::{Forge, Item};
use bureau::process::Secret;

/// The distinct credential references across the assignment's repos,
/// sorted so the first failure reported is deterministic.
fn credential_refs(config: &Config, assignment: &Assignment) -> BTreeSet<String> {
    assignment
        .repos
        .iter()
        .filter_map(|name| config.repos.get(name))
        .map(|repo| repo.credential.clone())
        .collect()
}

/// Resolves one reference into the map; a miss names it and fails.
fn resolve_one(
    credentials: &mut BTreeMap<String, Secret>,
    settings: &bureau::setup::Settings,
    reference: &str,
) -> Option<()> {
    match bureau::credential::resolve(settings, reference) {
        Ok(secret) => {
            credentials.insert(reference.to_owned(), secret);
            Some(())
        }
        Err(error) => {
            out::error(format_args!("{error}"));
            None
        }
    }
}

/// Every credential the assignment's repos need, resolved before any
/// spawn. A missing reference is named on stderr and fails the verb.
pub fn resolve_credentials(
    config: &Config,
    assignment: &Assignment,
    settings: &bureau::setup::Settings,
) -> Option<BTreeMap<String, Secret>> {
    let mut credentials = BTreeMap::new();
    for reference in credential_refs(config, assignment) {
        resolve_one(&mut credentials, settings, &reference)?;
    }
    Some(credentials)
}

/// The ADO organization root from the primary repo URL:
/// `https://dev.azure.com/org/project/_git/repo` becomes
/// `https://dev.azure.com/org`.
fn ado_base_url(repo_url: &str) -> String {
    let head = repo_url.split("/_git/").next().unwrap_or(repo_url);
    head.rsplit_once('/')
        .map_or_else(|| head.to_owned(), |(base, _)| base.to_owned())
}

/// The client for the forge the work items live on. v0: its token is the
/// PRIMARY repo's credential — the work forge shares that credential.
///
/// # Errors
/// Returns an error when the assignment has no primary repo.
pub fn work_forge(
    config: &Config,
    assignment: &Assignment,
    credentials: &BTreeMap<String, Secret>,
) -> anyhow::Result<Arc<dyn Forge>> {
    let primary = assignment
        .primary_repo()
        .and_then(|name| config.repos.get(name))
        .context("assignment has no primary repo")?;
    let token = credentials
        .get(&primary.credential)
        .cloned()
        .context("primary repo credential was not resolved")?;
    Ok(match assignment.work.forge {
        ForgeKind::Github => Arc::new(GitHubForge::new(token)),
        ForgeKind::Ado => Arc::new(AdoForge::new(ado_base_url(&primary.url), token)),
    })
}

/// Whether an external id names `query`: exactly (a `retry` reuses the
/// recorded id), or by `#<id>` / `/<id>` suffix — the id's repo context
/// is embedded (`owner/repo#42`, `project/42`).
fn matches_item(item: &Item, query: &str) -> bool {
    item.external_id == query
        || item.external_id.ends_with(&format!("#{query}"))
        || item.external_id.ends_with(&format!("/{query}"))
}

/// Queries the work forge and picks the item `query` names.
///
/// # Errors
/// Propagates forge query failures.
pub async fn find_item(
    forge: &dyn Forge,
    assignment: &Assignment,
    query: &str,
) -> anyhow::Result<Option<Item>> {
    let items = forge
        .query(&assignment.work.source, &assignment.work.filter)
        .await
        .context("querying the work forge")?;
    Ok(items.into_iter().find(|item| matches_item(item, query)))
}

/// The registry entries for exactly the assignment's repos.
pub fn plan_repos(config: &Config, assignment: &Assignment) -> BTreeMap<String, Repo> {
    config
        .repos
        .iter()
        .filter(|(name, _)| assignment.repos.contains(*name))
        .map(|(name, repo)| (name.clone(), repo.clone()))
        .collect()
}

/// The lease table's token for a forge kind.
pub const fn forge_name(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::Ado => "ado",
        ForgeKind::Github => "github",
    }
}
