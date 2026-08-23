//! GitHub issue dependency observation.

use reqwest::Method;
use serde::Deserialize;

use super::{Error, GitHubForge, Issue, json_body, repo_name, split_item_id};
use crate::forge::{Dependency, Item, LabelForge};

#[derive(Deserialize)]
struct BlockingIssue {
    number: u64,
    repository_url: String,
    state: String,
}

impl BlockingIssue {
    fn into_dependency(self) -> Result<Dependency, Error> {
        let repo = repo_name(&self.repository_url)?;
        Ok(Dependency {
            external_id: format!("{repo}#{}", self.number),
            closed: self.state == "closed",
        })
    }
}

pub(super) async fn blocking(forge: &GitHubForge, item_id: &str) -> Result<Vec<Dependency>, Error> {
    let (repo, number) = split_item_id(item_id)?;
    let url = format!(
        "{}/repos/{repo}/issues/{number}/dependencies/blocked_by",
        forge.base_url
    );
    let first = forge
        .request(Method::GET, &url)
        .query(&[("per_page", "100")]);
    let raw = forge
        .get_pages(first, |page: Vec<BlockingIssue>| page)
        .await?;
    raw.into_iter()
        .map(BlockingIssue::into_dependency)
        .collect()
}

#[async_trait::async_trait]
impl LabelForge for GitHubForge {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        <Self as crate::forge::Forge>::query(self, source, filter).await
    }

    async fn item(&self, item_id: &str) -> Result<Item, Error> {
        let (repo, number) = split_item_id(item_id)?;
        let url = format!("{}/repos/{repo}/issues/{number}", self.base_url);
        let issue: Issue = json_body(self.request(Method::GET, &url).send().await?).await?;
        issue
            .into_item(&repo)
            .ok_or_else(|| Error::Parse(format!("work item `{item_id}` changed repositories")))
    }

    async fn blocking_dependencies(&self, item_id: &str) -> Result<Vec<Dependency>, Error> {
        blocking(self, item_id).await
    }

    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error> {
        super::labels::update(self, item_id, add, remove).await
    }
}
