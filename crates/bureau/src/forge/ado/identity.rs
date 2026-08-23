//! Who a resolved credential is, according to Azure DevOps itself.

use serde::Deserialize;

use super::{AdoForge, Error, Identity, decode};
use crate::forge::identity::Reported;
use crate::process::Secret;

/// The connected account. `authenticatedUser` is required: a body
/// without it says nothing about who the credential is.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticatedUser {
    #[serde(default)]
    provider_display_name: String,
    #[serde(default)]
    properties: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionData {
    authenticated_user: AuthenticatedUser,
}

/// The account ADO records for the credential; its display name is the
/// fallback when the connection carries no account property. An empty
/// pair names nobody, so it answers `None`.
fn account(user: &AuthenticatedUser) -> Option<String> {
    let name = user.properties["Account"]["$value"]
        .as_str()
        .unwrap_or(&user.provider_display_name)
        .trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The account `credential` authenticates as: `_apis/connectionData`,
/// the organization-level call every personal access token may make.
/// A connection naming no account is an unexpected response, never a
/// verified empty identity.
pub(super) async fn get(forge: &AdoForge, credential: &Secret) -> Result<Reported, Error> {
    let url = forge.url("/_apis/connectionData?api-version=7.1");
    let request = forge.request_as(reqwest::Method::GET, &url, credential);
    let data: ConnectionData = decode(request.send().await?).await?;
    account(&data.authenticated_user)
        .map(|account| Reported::Account(Identity::new(account)))
        .ok_or_else(|| Error::Parse("connection data named no authenticated user".to_owned()))
}
