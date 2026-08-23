//! Who a resolved credential is, according to GitHub itself.

use reqwest::Method;
use serde::Deserialize;

use super::{Error, GitHubForge, Identity, json_body};
use crate::process::Secret;

#[derive(Deserialize)]
struct User {
    login: String,
}

/// The account `credential` authenticates as: `GET /user`, the one call
/// GitHub answers for any token, with no scope of its own.
pub(super) async fn get(
    forge: &GitHubForge,
    credential: &Secret,
) -> Result<Option<Identity>, Error> {
    let url = format!("{}/user", forge.base_url);
    let response = forge
        .request_as(Method::GET, &url, credential)
        .send()
        .await?;
    let user: User = json_body(response).await?;
    Ok(Some(Identity::new(user.login)))
}
