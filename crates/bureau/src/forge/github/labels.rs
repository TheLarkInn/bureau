//! Additive GitHub issue-label updates.

use reqwest::{Method, StatusCode, Url};

use super::{Error, GitHubForge, api_error, ensure_ok, split_item_id};

fn labels_url(forge: &GitHubForge, item_id: &str) -> Result<String, Error> {
    let (repo, number) = split_item_id(item_id)?;
    Ok(format!(
        "{}/repos/{repo}/issues/{number}/labels",
        forge.base_url
    ))
}

fn removal_url(base: &str, label: &str) -> Result<Url, Error> {
    let mut url = Url::parse(base).map_err(|error| Error::Parse(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|()| Error::Parse(format!("label URL cannot be extended: {base}")))?
        .push(label);
    Ok(url)
}

async fn post_add(
    forge: &GitHubForge,
    url: &str,
    labels: &[String],
) -> Result<reqwest::Response, Error> {
    Ok(forge
        .request(Method::POST, url)
        .json(&serde_json::json!({ "labels": labels }))
        .send()
        .await?)
}

async fn create(forge: &GitHubForge, item_id: &str, label: &str) -> Result<(), Error> {
    let (repo, _) = split_item_id(item_id)?;
    let url = format!("{}/repos/{repo}/labels", forge.base_url);
    let payload = serde_json::json!({
        "name": label,
        "color": "ededed",
        "description": "Bureau run terminal state",
    });
    let response = forge
        .request(Method::POST, &url)
        .json(&payload)
        .send()
        .await?;
    if response.status().is_success() || response.status() == StatusCode::UNPROCESSABLE_ENTITY {
        return Ok(());
    }
    Err(api_error(response.status(), &response.bytes().await?))
}

async fn provision(forge: &GitHubForge, item_id: &str, labels: &[String]) -> Result<(), Error> {
    for label in labels {
        create(forge, item_id, label).await?;
    }
    Ok(())
}

async fn provision_and_add(
    forge: &GitHubForge,
    item_id: &str,
    url: &str,
    labels: &[String],
) -> Result<(), Error> {
    provision(forge, item_id, labels).await?;
    ensure_ok(post_add(forge, url, labels).await?).await
}

async fn add(
    forge: &GitHubForge,
    item_id: &str,
    url: &str,
    labels: &[String],
) -> Result<(), Error> {
    if labels.is_empty() {
        return Ok(());
    }
    let response = post_add(forge, url, labels).await?;
    if response.status() == StatusCode::UNPROCESSABLE_ENTITY {
        return provision_and_add(forge, item_id, url, labels).await;
    }
    ensure_ok(response).await
}

pub(super) async fn set(
    forge: &GitHubForge,
    item_id: &str,
    labels: &[String],
) -> Result<(), Error> {
    let response = forge
        .request(Method::PUT, &labels_url(forge, item_id)?)
        .json(&serde_json::json!({ "labels": labels }))
        .send()
        .await?;
    ensure_ok(response).await
}

async fn remove(forge: &GitHubForge, url: &str, label: &str) -> Result<(), Error> {
    let response = forge
        .request(Method::DELETE, removal_url(url, label)?.as_str())
        .send()
        .await?;
    if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
        return Ok(());
    }
    Err(api_error(response.status(), &response.bytes().await?))
}

pub(super) async fn update(
    forge: &GitHubForge,
    item_id: &str,
    add_labels: &[String],
    remove_labels: &[String],
) -> Result<(), Error> {
    let url = labels_url(forge, item_id)?;
    add(forge, item_id, &url, add_labels).await?;
    for label in remove_labels {
        remove(forge, &url, label).await?;
    }
    Ok(())
}
