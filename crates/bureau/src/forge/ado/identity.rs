//! Who a resolved credential is, according to Azure DevOps itself.

use serde::Deserialize;

use super::{AdoForge, Error, Identity, decode};
use crate::process::Secret;

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct AuthenticatedUser {
    provider_display_name: String,
    properties: serde_json::Value,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ConnectionData {
    authenticated_user: AuthenticatedUser,
}

/// The account ADO records for the credential; its display name is the
/// fallback when the connection carries no account property.
fn account(user: &AuthenticatedUser) -> String {
    user.properties["Account"]["$value"]
        .as_str()
        .unwrap_or(&user.provider_display_name)
        .to_owned()
}

/// The account `credential` authenticates as: `_apis/connectionData`,
/// the organization-level call every personal access token may make.
pub(super) async fn get(forge: &AdoForge, credential: &Secret) -> Result<Option<Identity>, Error> {
    let url = forge.url("/_apis/connectionData?api-version=7.1");
    let request = forge.request_as(reqwest::Method::GET, &url, credential);
    let data: ConnectionData = decode(request.send().await?).await?;
    Ok(Some(Identity::new(account(&data.authenticated_user))))
}
