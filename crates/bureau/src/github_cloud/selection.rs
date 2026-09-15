use crate::config::{Access, ForgeKind, Repo};
use crate::forge::github::cloud::{AutomationId, Client, Principal, RepositoryRef};
use crate::process::Secret;
use crate::runlog::ConfigSource;

use super::{Error, Scope, State};

pub enum ExpectedIdentity {
    Login(String),
    Id(u64),
}

pub struct SelectionRequest {
    pub registry_name: String,
    pub repo: Repo,
    pub config_source: ConfigSource,
}

fn checked_repo(repo: &Repo) -> Result<RepositoryRef, Error> {
    if repo.forge != ForgeKind::Github {
        return Err(Error::Selection(
            "the registered repository is not GitHub".to_owned(),
        ));
    }
    Ok(RepositoryRef::parse(&repo.url)?)
}

async fn principal(client: &Client, expected: &ExpectedIdentity) -> Result<Principal, Error> {
    Ok(match expected {
        ExpectedIdentity::Login(login) => client.authenticate(login).await?,
        ExpectedIdentity::Id(id) => client.authenticate_id(*id).await?,
    })
}

fn scope(request: &SelectionRequest, repo: &RepositoryRef, principal: Principal) -> Scope {
    Scope {
        repo: repo.name(),
        registry_name: request.registry_name.clone(),
        credential_reference: request.repo.credential.clone(),
        principal_id: principal.id,
        principal_login: principal.login,
        config_source: request.config_source.clone(),
    }
}

fn same_authority(earlier: &Scope, current: &Scope) -> bool {
    earlier.registry_name == current.registry_name
        && earlier.credential_reference == current.credential_reference
        && earlier.config_source.remote == current.config_source.remote
        && earlier.config_source.reference == current.config_source.reference
}

pub struct Selection {
    scope: Scope,
    repo: RepositoryRef,
    dispatch_allowed: bool,
    credential: Secret,
}

impl Selection {
    #[must_use]
    pub const fn scope(&self) -> &Scope {
        &self.scope
    }

    #[must_use]
    pub const fn repo(&self) -> &RepositoryRef {
        &self.repo
    }

    pub(super) fn check_client(&self, client: &Client) -> Result<(), Error> {
        if client.matches_credential(&self.credential) {
            Ok(())
        } else {
            Err(Error::Selection(
                "client credential differs from the verified selection".to_owned(),
            ))
        }
    }

    pub(super) fn require_dispatch(&self) -> Result<(), Error> {
        if self.dispatch_allowed {
            Ok(())
        } else {
            Err(Error::Unsupported(
                "dispatch requires reviewed repository access: push; no grant was changed"
                    .to_owned(),
            ))
        }
    }

    pub(super) fn check_binding(
        &self,
        state: &State,
        automation: &AutomationId,
    ) -> Result<(), Error> {
        let earlier = &state.start.scope;
        let matches = earlier.repo == self.scope.repo
            && earlier.principal_id == self.scope.principal_id
            && same_authority(earlier, &self.scope)
            && state.start.automation_id == automation.as_str();
        if matches {
            Ok(())
        } else {
            Err(Error::Selection("request key belongs to a different repository, automation, credential, principal, or config authority".to_owned()))
        }
    }
}

/// Selects a committed registry entry and verifies its credential's principal.
///
/// # Errors
/// Rejects unsupported repositories, authentication failures, and identity mismatch.
pub async fn select(
    client: &Client,
    request: &SelectionRequest,
    expected: &ExpectedIdentity,
) -> Result<Selection, Error> {
    let repo = checked_repo(&request.repo)?;
    let principal = principal(client, expected).await?;
    Ok(Selection {
        scope: scope(request, &repo, principal),
        repo,
        dispatch_allowed: request.repo.access == Access::Push,
        credential: client.credential(),
    })
}
