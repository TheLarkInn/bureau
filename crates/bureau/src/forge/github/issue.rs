//! GitHub issue mapping and single-issue reads.

use reqwest::Method;
use serde::Deserialize;

use super::{Error, GitHubForge, json_body, repo_name, split_item_id};
use crate::contract::Trust;
use crate::forge::Item;

fn trust(association: &str) -> Trust {
    match association {
        "OWNER" | "MEMBER" | "COLLABORATOR" => Trust::Maintainer,
        _ => Trust::Untrusted,
    }
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
pub(super) struct Issue {
    number: u64,
    repository_url: String,
    title: String,
    body: Option<String>,
    html_url: String,
    labels: Vec<Label>,
    author_association: String,
}

impl Issue {
    pub(super) fn into_item(self, expected_repo: &str) -> Option<Item> {
        let repo = repo_name(&self.repository_url).ok()?;
        if !repo.eq_ignore_ascii_case(expected_repo) {
            return None;
        }
        Some(Item {
            external_id: format!("{repo}#{}", self.number),
            title: self.title,
            body: self.body.unwrap_or_default(),
            url: self.html_url,
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            trust: trust(&self.author_association),
        })
    }
}

/// Reads one issue; a transferred issue is refused rather than followed.
pub(super) async fn read(forge: &GitHubForge, item_id: &str) -> Result<Item, Error> {
    let (repo, number) = split_item_id(item_id)?;
    let url = format!("{}/repos/{repo}/issues/{number}", forge.base_url);
    let issue: Issue = json_body(forge.request(Method::GET, &url).send().await?).await?;
    issue
        .into_item(&repo)
        .ok_or_else(|| Error::Parse(format!("work item `{item_id}` changed repositories")))
}
