//! Where the reviewed config remote is addressed, and with which
//! credential.
//!
//! The runner clones that remote with the reserved config credential
//! before any assignment config exists, so no registry entry names a
//! host for it. Shaping the remote as a [`Repo`] borrows the registry's
//! own host derivation rather than parsing the URL a second way.

use super::{Access, CONFIG_CREDENTIAL, ForgeKind, Repo};

/// The forge a config remote implies: Azure DevOps names itself in the
/// URL, and everything else is GitHub. The reconcile daemon's
/// `--config-forge` flag overrides this; omitting it asks here.
#[must_use]
pub fn config_forge(remote: &str) -> ForgeKind {
    if remote.contains("dev.azure.com") || remote.contains("/_git/") {
        ForgeKind::Ado
    } else {
        ForgeKind::Github
    }
}

/// The registry-shaped entry the config remote implies: read-only, on
/// `forge`, referencing the reserved config credential. It is what
/// authorizes exactly one host to answer for that credential.
#[must_use]
pub fn config_repo(remote: &str, forge: ForgeKind) -> Repo {
    Repo {
        url: remote.to_owned(),
        forge,
        access: Access::Read,
        credential: CONFIG_CREDENTIAL.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{CONFIG_CREDENTIAL, config_forge, config_repo};

    /// The one host each kind of config remote authorizes, derived by
    /// the same rule a registered repo's host is.
    #[test]
    fn a_config_remote_is_addressed_at_the_root_it_implies() {
        let hosts = [
            "https://github.com/acme/config.git",
            "https://ghe.acme.example/acme/config",
            "https://dev.azure.com/acme/config/_git/config",
        ]
        .map(|remote| config_repo(remote, config_forge(remote)).forge_host());
        assert_eq!(
            hosts,
            [
                "github https://api.github.com".to_owned(),
                "github https://ghe.acme.example/api/v3".to_owned(),
                "ado https://dev.azure.com/acme".to_owned(),
            ]
        );
    }

    /// The entry names the reserved reference, so the value offered to
    /// that host is the one the config fetch will sign with.
    #[test]
    fn a_config_remote_names_the_reserved_credential() {
        let repo = config_repo("https://github.com/acme/config", config_forge("x"));
        assert_eq!(repo.credential, CONFIG_CREDENTIAL);
    }
}
