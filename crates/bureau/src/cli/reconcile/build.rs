//! Credentials and per-assignment forge clients for one config revision.

use crate::cli::out;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::Context as _;
use bureau::config::{Assignment, Config, ForgeKind, LabelRule, Repo};
use bureau::forge::ado::AdoForge;
use bureau::forge::github::GitHubForge;
use bureau::forge::{Forge, LabelForge};
use bureau::git::{Credential, credential_for};
use bureau::process::{Secret, resolve};

use super::ForgeArg;

fn resolve_reference(
    settings: Option<&bureau::setup::Settings>,
    reference: &str,
) -> anyhow::Result<Secret> {
    settings.map_or_else(
        || resolve(reference).map_err(anyhow::Error::from),
        |settings| bureau::credential::resolve(settings, reference).map_err(anyhow::Error::from),
    )
}

fn ado_base_url(repo_url: &str) -> String {
    let head = repo_url.split("/_git/").next().unwrap_or(repo_url);
    head.rsplit_once('/')
        .map_or_else(|| head.to_owned(), |(base, _)| base.to_owned())
}

fn resolve_optional(
    settings: Option<&bureau::setup::Settings>,
    reference: &str,
) -> Option<(String, Secret)> {
    match resolve_reference(settings, reference) {
        Ok(secret) => Some((reference.to_owned(), secret)),
        Err(error) => {
            out::error(format_args!(
                "credential `{reference}` is unavailable: {error}"
            ));
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

fn label_forge(
    rule: &LabelRule,
    repos: &BTreeMap<String, Repo>,
    credentials: &BTreeMap<String, Secret>,
) -> anyhow::Result<Arc<dyn LabelForge>> {
    let repo = rule
        .source_repo(repos)
        .context("label rule source has no registered repo")?;
    let token = credentials
        .get(&repo.credential)
        .cloned()
        .context("label rule repo credential was not resolved")?;
    match rule.work.forge {
        ForgeKind::Github => Ok(Arc::new(GitHubForge::for_repo(token, &repo.url)?)),
        ForgeKind::Ado => anyhow::bail!("label rules currently require GitHub"),
    }
}

pub(super) fn config_credential(
    reference: Option<&str>,
    forge: ForgeArg,
    settings: Option<&bureau::setup::Settings>,
) -> anyhow::Result<Option<Credential>> {
    reference
        .map(|reference| {
            let secret = resolve_reference(settings, reference)
                .with_context(|| format!("resolving config credential reference `{reference}`"))?;
            Ok(credential_for(forge.into(), secret))
        })
        .transpose()
}

pub(super) fn credentials(
    config: &Config,
    settings: Option<&bureau::setup::Settings>,
) -> BTreeMap<String, Secret> {
    let references: BTreeSet<_> = config
        .repos
        .values()
        .map(|repo| repo.credential.as_str())
        .collect();
    references
        .into_iter()
        .filter_map(|reference| resolve_optional(settings, reference))
        .collect()
}

/// The identity each declared credential must authenticate as. Without
/// settings there is no declaration to enforce, so nothing is expected.
pub(super) fn identities(settings: Option<&bureau::setup::Settings>) -> BTreeMap<String, String> {
    settings.map_or_else(BTreeMap::new, bureau::setup::Settings::declared_identities)
}

pub(super) fn credentials_for_repos(
    repos: &BTreeMap<String, Repo>,
    settings: Option<&bureau::setup::Settings>,
) -> anyhow::Result<BTreeMap<String, Secret>> {
    let references: BTreeSet<_> = repos
        .values()
        .map(|repo| repo.credential.as_str())
        .collect();
    references
        .into_iter()
        .map(|reference| {
            let secret = resolve_reference(settings, reference)
                .with_context(|| format!("resolving credential reference `{reference}`"))?;
            Ok((reference.to_owned(), secret))
        })
        .collect()
}

pub(super) fn forges(
    config: &Config,
    credentials: &BTreeMap<String, Secret>,
) -> BTreeMap<String, Arc<dyn Forge>> {
    let forges: BTreeMap<String, Arc<dyn Forge>> = config
        .assignments
        .iter()
        .filter_map(|(name, assignment)| {
            forge(assignment, &config.repos, credentials)
                .map(|forge| (name.clone(), forge))
                .map_err(|error| {
                    out::error(format_args!(
                        "assignment `{name}` forge is unavailable: {error}"
                    ));
                })
                .ok()
        })
        .collect();
    forges
}

pub(super) fn label_forges(
    config: &Config,
    credentials: &BTreeMap<String, Secret>,
) -> BTreeMap<String, Arc<dyn LabelForge>> {
    config
        .label_rules
        .iter()
        .filter_map(|(name, rule)| {
            label_forge(rule, &config.repos, credentials)
                .map(|forge| (name.clone(), forge))
                .map_err(|error| {
                    out::error(format_args!(
                        "label rule `{name}` forge is unavailable: {error}"
                    ));
                })
                .ok()
        })
        .collect()
}
