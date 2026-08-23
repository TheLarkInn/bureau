//! Which host may be asked about which credential.
//!
//! A credential's identity is only ever checked against a host one of
//! the run's own registered repos points at: the work forge's client is
//! not authorized for a context repo's credential, and an Azure DevOps
//! or GitHub Enterprise value never travels to another organization's
//! root. One client per distinct host keeps the check to one call.

use std::collections::{BTreeMap, BTreeSet};

use bureau::config::Repo;
use bureau::forge::identity::{Authorizations, Authorized};
use bureau::process::Secret;

use super::repo_forge;

/// One repo's authorized host, when its credential resolved and its URL
/// names a forge this runner can address.
fn authorized(repo: &Repo, credentials: &BTreeMap<String, Secret>) -> Option<Authorized> {
    let token = credentials.get(&repo.credential)?.clone();
    let client = repo_forge(repo, token).ok()?;
    Some(Authorized {
        host: repo.forge_host(),
        client,
    })
}

/// The hosts authorized to verify each credential these repos
/// reference. A reference with no resolvable client is absent, and the
/// run then fails closed for it rather than ask an unauthorized host.
pub fn authorizations(
    repos: &BTreeMap<String, Repo>,
    credentials: &BTreeMap<String, Secret>,
) -> Authorizations {
    let mut authorizations = Authorizations::new();
    let mut seen = BTreeSet::new();
    for repo in repos.values() {
        let Some(entry) = authorized(repo, credentials) else {
            continue;
        };
        if seen.insert((repo.credential.clone(), entry.host.clone())) {
            let hosts = authorizations.entry(repo.credential.clone()).or_default();
            hosts.push(entry);
        }
    }
    authorizations
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, Repo, Secret, authorizations};
    use bureau::config::{Access, ForgeKind};

    fn repo(url: &str, forge: ForgeKind, credential: &str) -> Repo {
        Repo {
            url: url.to_owned(),
            forge,
            access: Access::Push,
            credential: credential.to_owned(),
        }
    }

    /// Two GitHub repos sharing a credential and a host, one Azure
    /// DevOps repo with its own.
    fn registry() -> BTreeMap<String, Repo> {
        BTreeMap::from([
            (
                "web".to_owned(),
                repo("https://github.com/acme/web", ForgeKind::Github, "gh-main"),
            ),
            (
                "docs".to_owned(),
                repo("https://github.com/acme/docs", ForgeKind::Github, "gh-main"),
            ),
            (
                "svc".to_owned(),
                repo(
                    "https://dev.azure.com/acme/svc/_git/svc",
                    ForgeKind::Ado,
                    "ado-main",
                ),
            ),
        ])
    }

    fn credentials() -> BTreeMap<String, Secret> {
        BTreeMap::from([
            ("gh-main".to_owned(), Secret::new("gh")),
            ("ado-main".to_owned(), Secret::new("ado")),
        ])
    }

    /// Each reference is authorized for its own hosts only, and two
    /// repos sharing a host share one client rather than two calls.
    #[test]
    fn each_credential_is_authorized_only_for_the_hosts_its_repos_name() {
        let authorized = authorizations(&registry(), &credentials());
        let hosts: Vec<(&str, &str)> = authorized
            .iter()
            .flat_map(|(reference, entries)| {
                entries
                    .iter()
                    .map(move |entry| (reference.as_str(), entry.host.as_str()))
            })
            .collect();
        assert_eq!(
            hosts,
            vec![
                ("ado-main", "ado https://dev.azure.com/acme"),
                ("gh-main", "github https://api.github.com"),
            ]
        );
    }

    /// An unresolved reference authorizes nothing: a value that does
    /// not exist is never carried to a host to be checked.
    #[test]
    fn an_unresolved_reference_authorizes_no_host() {
        let only_ado = BTreeMap::from([("ado-main".to_owned(), Secret::new("ado"))]);
        let authorized = authorizations(&registry(), &only_ado);
        let references: Vec<&str> = authorized.keys().map(String::as_str).collect();
        assert_eq!(references, vec!["ado-main"]);
    }
}
