use crate::forge::{Error, PrStatus};
use serde::Deserialize;

use super::{GitHubForge, json_body, repo_name};

#[derive(Deserialize)]
struct PullStatus {
    #[serde(default)]
    state: String,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    merge_commit_sha: Option<String>,
}

pub(super) async fn get(forge: &GitHubForge, repo: &str, number: u64) -> Result<PrStatus, Error> {
    let repo = repo_name(repo)?;
    let url = format!("{}/repos/{repo}/pulls/{number}", forge.base_url);
    let response = forge.request(reqwest::Method::GET, &url).send().await?;
    let status = json_body::<PullStatus>(response).await?;
    Ok(status.into_status())
}

impl PullStatus {
    fn into_status(self) -> PrStatus {
        if self.merged_at.is_some() {
            PrStatus::Merged {
                commit: self.merge_commit_sha,
            }
        } else if self.state == "closed" {
            PrStatus::Closed
        } else {
            PrStatus::Open
        }
    }
}
