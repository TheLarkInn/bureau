//! Where a registered repo's forge is addressed, and therefore which
//! host a credential named by that repo may be sent to.
//!
//! Two repos that differ here are two hosts: an Azure DevOps
//! organization, a GitHub Enterprise root, and `api.github.com` are
//! separate destinations even when one credential reference names them
//! all.

use super::Repo;
use crate::forge::ForgeKind;

/// The Azure DevOps organization root a repo URL implies:
/// `https://dev.azure.com/org/project/_git/repo` is addressed at
/// `https://dev.azure.com/org`.
fn ado_root(repo_url: &str) -> String {
    let head = repo_url.split("/_git/").next().unwrap_or(repo_url);
    head.rsplit_once('/')
        .map_or_else(|| head.to_owned(), |(base, _)| base.to_owned())
}

/// The GitHub API root a repo URL implies: `api.github.com` for
/// `github.com`, and the enterprise `/api/v3` root otherwise.
fn github_root(repo_url: &str) -> String {
    crate::forge::repository::parse(repo_url)
        .map_or_else(|| repo_url.to_owned(), |location| location.api_url())
}

impl Repo {
    /// The API root this repo's forge is addressed at.
    #[must_use]
    pub fn api_root(&self) -> String {
        match self.forge {
            ForgeKind::Github => github_root(&self.url),
            ForgeKind::Ado => ado_root(&self.url),
        }
    }

    /// The host a credential this repo names may be sent to: the forge
    /// kind and the API root together, because the same root under two
    /// forges is two destinations.
    #[must_use]
    pub fn forge_host(&self) -> String {
        let kind = match self.forge {
            ForgeKind::Github => "github",
            ForgeKind::Ado => "ado",
        };
        format!("{kind} {}", self.api_root())
    }
}

#[cfg(test)]
mod tests {
    use super::{ForgeKind, Repo};
    use crate::config::Access;

    fn repo(url: &str, forge: ForgeKind) -> Repo {
        Repo {
            url: url.to_owned(),
            forge,
            access: Access::Read,
            credential: "shared".to_owned(),
        }
    }

    /// Each forge's root, and the enterprise and organization roots that
    /// make two repos on one forge two separate hosts.
    #[test]
    fn each_repo_is_addressed_at_the_root_its_url_implies() {
        let roots = [
            repo("https://github.com/acme/web", ForgeKind::Github),
            repo("https://ghe.acme.example/acme/web", ForgeKind::Github),
            repo("https://dev.azure.com/acme/svc/_git/svc", ForgeKind::Ado),
        ]
        .map(|repo| repo.forge_host());
        assert_eq!(
            roots,
            [
                "github https://api.github.com".to_owned(),
                "github https://ghe.acme.example/api/v3".to_owned(),
                "ado https://dev.azure.com/acme".to_owned(),
            ]
        );
    }
}
