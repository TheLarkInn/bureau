//! Who a resolved credential is, according to GitHub itself.

use reqwest::{Method, StatusCode};
use serde::Deserialize;

use super::{Error, GitHubForge, Identity, json_body, response_error};
use crate::forge::identity::Reported;
use crate::process::Secret;

#[derive(Deserialize)]
struct User {
    login: String,
}

/// A 403 from `/user` is not proof the value is bad: GitHub documents
/// that call as unavailable to a GitHub App installation token, which
/// is otherwise valid. One read-only listing settles whether the value
/// works, and it stays nameless — an installation has no account.
async fn installation(forge: &GitHubForge, credential: &Secret) -> Result<Reported, Error> {
    let url = format!("{}/installation/repositories?per_page=1", forge.base_url);
    let response = forge
        .request_as(Method::GET, &url, credential)
        .send()
        .await?;
    if response.status().is_success() {
        return Ok(Reported::Unnamed);
    }
    Err(response_error(response).await?)
}

/// The login the response names, or the installation fallback when
/// GitHub answered 403.
async fn named(
    forge: &GitHubForge,
    credential: &Secret,
    response: reqwest::Response,
) -> Result<Reported, Error> {
    if response.status() == StatusCode::FORBIDDEN {
        return installation(forge, credential).await;
    }
    let user: User = json_body(response).await?;
    Ok(Reported::Account(Identity::new(user.login)))
}

/// The account `credential` authenticates as: `GET /user`, the one call
/// GitHub answers for any user token, with no scope of its own.
pub(super) async fn get(forge: &GitHubForge, credential: &Secret) -> Result<Reported, Error> {
    let url = format!("{}/user", forge.base_url);
    let response = forge
        .request_as(Method::GET, &url, credential)
        .send()
        .await?;
    named(forge, credential, response).await
}
