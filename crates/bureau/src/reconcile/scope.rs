//! What one claimed run takes from a reconcile pass: exactly the
//! credentials its own repos name, and exactly the hosts those repos
//! authorize for them.
//!
//! The pass resolves credentials and authorizes hosts for the whole
//! config. A run may see neither in full: another assignment's repo can
//! name the same credential on another host, and that host is not one
//! this run's repos point at, so it is dropped here rather than asked.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::Repo;
use crate::forge::identity::{Authorizations, Authorized};
use crate::process::Secret;

/// The resolved credentials these repos reference. An unresolvable
/// reference is left out: the engine escalates at push time instead.
pub(super) fn credentials(
    repos: &BTreeMap<String, Repo>,
    resolved: &BTreeMap<String, Secret>,
) -> BTreeMap<String, Secret> {
    repos
        .values()
        .filter_map(|repo| {
            let secret = resolved.get(&repo.credential)?.clone();
            Some((repo.credential.clone(), secret))
        })
        .collect()
}

/// The exact host these repos authorize for one reference.
fn host_of(reference: &str, authorized: &Authorizations, host: &str) -> Option<Authorized> {
    authorized
        .get(reference)
        .and_then(|entries| entries.iter().find(|entry| entry.host == host))
        .cloned()
}

/// The authorized hosts for exactly these repos' credentials, so a run
/// never offers a value to a host outside the repos it runs against.
pub(super) fn identity_forges(
    repos: &BTreeMap<String, Repo>,
    authorized: &Authorizations,
) -> Authorizations {
    let mut scoped = Authorizations::new();
    let mut seen = BTreeSet::new();
    for repo in repos.values() {
        let key = (repo.credential.clone(), repo.forge_host());
        if seen.insert(key.clone())
            && let Some(entry) = host_of(&key.0, authorized, &key.1)
        {
            scoped.entry(key.0).or_default().push(entry);
        }
    }
    scoped
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Authorizations, Authorized, BTreeMap, Repo, Secret, credentials, identity_forges};
    use crate::config::{Access, ForgeKind};
    use crate::forge::fake::FakeForge;

    fn repo(url: &str, credential: &str) -> Repo {
        Repo {
            url: url.to_owned(),
            forge: ForgeKind::Github,
            access: Access::Push,
            credential: credential.to_owned(),
        }
    }

    fn authorized(hosts: &[&str]) -> Vec<Authorized> {
        hosts
            .iter()
            .map(|host| Authorized {
                host: (*host).to_owned(),
                client: Arc::new(FakeForge::default()),
            })
            .collect()
    }

    /// One credential authorized on two hosts, plus a second credential
    /// no repo in the run names.
    fn everything_authorized() -> Authorizations {
        Authorizations::from([
            (
                "shared".to_owned(),
                authorized(&["github https://api.github.com", "github https://ghe/api/v3"]),
            ),
            (
                "other".to_owned(),
                authorized(&["github https://ghe/api/v3"]),
            ),
        ])
    }

    /// This run's own repos: one on `shared` at `api.github.com`, one
    /// on `other` at the Enterprise host.
    fn run_repos() -> BTreeMap<String, Repo> {
        BTreeMap::from([
            (
                "main".to_owned(),
                repo("https://github.com/acme/web", "shared"),
            ),
            (
                "internal".to_owned(),
                repo("https://ghe/acme/internal", "other"),
            ),
        ])
    }

    /// Every (reference, host) pair the run was handed.
    fn pairs(taken: &Authorizations) -> Vec<(String, String)> {
        taken
            .iter()
            .flat_map(|(reference, entries)| {
                entries
                    .iter()
                    .map(move |entry| (reference.clone(), entry.host.clone()))
            })
            .collect()
    }

    /// A run takes exactly the reference-host pairs its own repos name.
    /// `shared` is authorized on the Enterprise host too, and only the
    /// `other` credential's repo points there, so that pair is dropped
    /// rather than handed over with it.
    #[test]
    fn a_run_takes_only_the_credentials_and_hosts_its_repos_name() {
        let taken = identity_forges(&run_repos(), &everything_authorized());
        assert_eq!(
            pairs(&taken),
            vec![
                ("other".to_owned(), "github https://ghe/api/v3".to_owned()),
                (
                    "shared".to_owned(),
                    "github https://api.github.com".to_owned()
                ),
            ]
        );
    }

    /// Resolution is scoped the same way: a value no repo in this run
    /// names is not carried into it at all.
    #[test]
    fn a_run_resolves_only_the_credentials_its_repos_name() {
        let resolved = BTreeMap::from([
            ("shared".to_owned(), Secret::new("mine")),
            ("other".to_owned(), Secret::new("theirs")),
            ("elsewhere".to_owned(), Secret::new("nobody")),
        ]);
        let references: Vec<String> = credentials(&run_repos(), &resolved).into_keys().collect();
        assert_eq!(references, vec!["other".to_owned(), "shared".to_owned()]);
    }
}
