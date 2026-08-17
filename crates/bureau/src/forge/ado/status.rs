use crate::forge::{Error, PrStatus};
use serde::Deserialize;

use super::{AdoForge, decode, repo_parts};

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct RawPr {
    pub(super) pull_request_id: u64,
    pub(super) source_ref_name: String,
    pub(super) title: String,
    pub(super) url: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawCommit {
    commit_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawStatus {
    status: String,
    last_merge_commit: Option<RawCommit>,
}

impl RawStatus {
    fn into_status(self) -> PrStatus {
        match self.status.as_str() {
            "completed" => PrStatus::Merged {
                commit: self.last_merge_commit.map(|commit| commit.commit_id),
            },
            "abandoned" => PrStatus::Closed,
            _ => PrStatus::Open,
        }
    }
}

pub(super) async fn get(forge: &AdoForge, repo: &str, number: u64) -> Result<PrStatus, Error> {
    let (project, repo) = repo_parts(repo)?;
    let url = forge.url(&format!(
        "/{project}/_apis/git/repositories/{repo}/pullrequests/{number}?api-version=7.1"
    ));
    let raw: RawStatus = decode(forge.get(&url).send().await?).await?;
    Ok(raw.into_status())
}
