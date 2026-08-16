//! Credentials and per-assignment forge clients for one config revision.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::{Assignment, Config, ForgeKind, Repo};
use bureau::forge::Forge;
use bureau::forge::ado::AdoForge;
use bureau::forge::github::GitHubForge;
use bureau::git::{Credential, credential_for};
use bureau::process::{Secret, resolve};

use super::ForgeArg;

pub(super) fn config_credential(
    reference: Option<&str>,
    forge: ForgeArg,
) -> anyhow::Result<Option<Credential>> {
    reference
        .map(|reference| {
            let secret = resolve(reference)
                .with_context(|| format!("resolving config credential reference `{reference}`"))?;
            Ok(credential_for(forge.into(), secret))
        })
        .transpose()
}

pub(super) fn credentials(config: &Config) -> BTreeMap<String, Secret> {
    let references: BTreeSet<_> = config
        .repos
        .values()
        .map(|repo| repo.credential.as_str())
        .collect();
    references
        .into_iter()
        .filter_map(resolve_optional)
        .collect()
}

pub(super) fn credentials_for_repos(
    repos: &BTreeMap<String, Repo>,
) -> anyhow::Result<BTreeMap<String, Secret>> {
    let references: BTreeSet<_> = repos
        .values()
        .map(|repo| repo.credential.as_str())
        .collect();
    references
        .into_iter()
        .map(|reference| {
            let secret = resolve(reference)
                .with_context(|| format!("resolving credential reference `{reference}`"))?;
            Ok((reference.to_owned(), secret))
        })
        .collect()
}

pub(super) fn forges(
    config: &Config,
    credentials: &BTreeMap<String, Secret>,
) -> BTreeMap<String, Arc<dyn Forge>> {
    config
        .assignments
        .iter()
        .filter_map(|(name, assignment)| {
            forge(assignment, &config.repos, credentials)
                .map(|forge| (name.clone(), forge))
                .map_err(|error| {
                    eprintln!("assignment `{name}` forge is unavailable: {error}");
                })
                .ok()
        })
        .collect()
}

fn resolve_optional(reference: &str) -> Option<(String, Secret)> {
    match resolve(reference) {
        Ok(secret) => Some((reference.to_owned(), secret)),
        Err(error) => {
            eprintln!("credential `{reference}` is unavailable: {error}");
            None
        }
    }
}

pub(super) fn forge(
    assignment: &Assignment,
    repos: &BTreeMap<String, Repo>,
    credentials: &BTreeMap<String, Secret>,
) -> anyhow::Result<Arc<dyn Forge>> {
    let primary = assignment
        .primary_repo()
        .and_then(|name| repos.get(name))
        .context("assignment has no registered primary repo")?;
    let token = credentials
        .get(&primary.credential)
        .cloned()
        .context("primary repo credential was not resolved")?;
    Ok(match assignment.work.forge {
        ForgeKind::Github => Arc::new(GitHubForge::new(token)),
        ForgeKind::Ado => Arc::new(AdoForge::new(ado_base_url(&primary.url), token)),
    })
}

fn ado_base_url(repo_url: &str) -> String {
    let head = repo_url.split("/_git/").next().unwrap_or(repo_url);
    head.rsplit_once('/')
        .map_or_else(|| head.to_owned(), |(base, _)| base.to_owned())
}
