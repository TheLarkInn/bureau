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

/// The hosts these repos authorize for one reference, and no others.
fn hosts_of(
    reference: &str,
    authorized: &Authorizations,
    hosts: &BTreeSet<String>,
) -> Vec<Authorized> {
    authorized
        .get(reference)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| hosts.contains(&entry.host))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// The authorized hosts for exactly these repos' credentials, so a run
/// never offers a value to a host outside the repos it runs against.
pub(super) fn identity_forges(
    repos: &BTreeMap<String, Repo>,
    authorized: &Authorizations,
) -> Authorizations {
    let hosts: BTreeSet<String> = repos.values().map(Repo::forge_host).collect();
    repos
        .values()
        .map(|repo| {
            let reference = repo.credential.clone();
            let entries = hosts_of(&reference, authorized, &hosts);
            (reference, entries)
        })
        .collect()
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

    /// A run takes only what its own repos name: another assignment's
    /// credential stays out entirely, and a host only that other repo
    /// points at is dropped even when it shares the credential.
    #[test]
    fn a_run_takes_only_the_credentials_and_hosts_its_repos_name() {
        let repos = BTreeMap::from([(
            "main".to_owned(),
            repo("https://github.com/acme/web", "shared"),
        )]);
        let resolved = BTreeMap::from([
            ("shared".to_owned(), Secret::new("mine")),
            ("other".to_owned(), Secret::new("theirs")),
        ]);
        let taken = identity_forges(&repos, &everything_authorized());
        let hosts: Vec<&str> = taken["shared"]
            .iter()
            .map(|one| one.host.as_str())
            .collect();
        let references: Vec<String> = credentials(&repos, &resolved).into_keys().collect();
        assert_eq!(
            (taken.len(), hosts, references),
            (
                1,
                vec!["github https://api.github.com"],
                vec!["shared".to_owned()]
            )
        );
    }
}
