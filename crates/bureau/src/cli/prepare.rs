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

/// The client for one registered repo, signed with `token`: the same
/// forge the repo lives on, at the API root its URL implies.
///
/// # Errors
/// Returns an error when the repo URL is not a supported reference.
pub fn repo_forge(repo: &Repo, token: Secret) -> anyhow::Result<Arc<dyn Forge>> {
    Ok(match repo.forge {
        ForgeKind::Github => Arc::new(GitHubForge::for_repo(token, &repo.url)?),
        ForgeKind::Ado => Arc::new(AdoForge::new(ado_base_url(&repo.url), token)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use bureau::config::{Access, Limits, WorkSource};

    fn repo(credential: &str) -> Repo {
        Repo {
            url: "https://github.com/acme/web".to_owned(),
            forge: ForgeKind::Github,
            access: Access::Push,
            credential: credential.to_owned(),
        }
    }

    fn config() -> Config {
        Config {
            repos: BTreeMap::from([
                ("primary".to_owned(), repo("primary-ref")),
                ("context".to_owned(), repo("context-ref")),
            ]),
            roles: BTreeMap::new(),
            assignments: BTreeMap::new(),
            label_rules: BTreeMap::new(),
            pipelines: BTreeMap::new(),
        }
    }

    fn assignment(repos: &[&str]) -> Assignment {
        Assignment {
            name: "fix".to_owned(),
            work: WorkSource {
                forge: ForgeKind::Github,
                source: "acme/web".to_owned(),
                filter: "*".to_owned(),
                approval_label: None,
                abort_label: "bureau:failed".to_owned(),
                escalate_label: "bureau:needs-human".to_owned(),
            },
            repos: repos.iter().map(|name| (*name).to_owned()).collect(),
            pipeline: "fix".to_owned(),
            role: "worker".to_owned(),
            verify: "true".to_owned(),
            branch_prefix: "bureau/".to_owned(),
            limits: Limits::default(),
        }
    }

    fn credentials(names: &[&str]) -> BTreeMap<String, Secret> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), Secret::new("token")))
            .collect()
    }

    /// README's known delta, pinned: the work forge's token is the
    /// PRIMARY repo's credential — a resolved primary credential alone
    /// suffices, even with a context repo's reference unresolved.
    #[test]
    fn the_work_forge_token_is_the_primary_repos_credential() {
        let built = work_forge(
            &config(),
            &assignment(&["primary", "context"]),
            &credentials(&["primary-ref"]),
        );
        assert!(built.is_ok(), "primary credential alone builds the forge");
    }

    /// The flip side: a context repo's credential never substitutes for
    /// the primary's, and the error says so.
    #[test]
    fn a_context_credential_cannot_substitute_for_the_primary() {
        let error = work_forge(
            &config(),
            &assignment(&["primary", "context"]),
            &credentials(&["context-ref"]),
        )
        .err()
        .expect("context credential must not build the work forge");
        assert!(
            error.to_string().contains("primary repo credential"),
            "error names the rule: {error}"
        );
    }

    /// No primary repo (an empty `repos` list) fails closed as well.
    #[test]
    fn an_assignment_without_a_primary_repo_is_rejected() {
        let error = work_forge(&config(), &assignment(&[]), &credentials(&[]))
            .err()
            .expect("no primary repo must fail");
        assert!(
            error.to_string().contains("no primary repo"),
            "error names the rule: {error}"
        );
    }
}
