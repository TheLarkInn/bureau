//! GitHub forge: REST/GraphQL over reqwest with rustls. Never shells out
//! to `gh` (DESIGN.md layer 7).

use async_trait::async_trait;

use super::{Error, Forge, Item, Pr, PrRequest};
use crate::process::Secret;

/// GitHub API client. `source`/`repo` arguments use `owner/name` form.
pub struct GitHubForge {
    /// API root; `https://api.github.com` in production, a local test
    /// server in tests.
    pub base_url: String,
    token: Secret,
    client: reqwest::Client,
}

impl GitHubForge {
    /// A client against `https://api.github.com`.
    #[must_use]
    pub fn new(token: Secret) -> Self {
        Self {
            base_url: "https://api.github.com".to_owned(),
            token,
            client: reqwest::Client::new(),
        }
    }

    /// Points the client at another API root (tests, GitHub Enterprise).
    #[must_use]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }
}

#[async_trait]
impl Forge for GitHubForge {
    async fn query(&self, _source: &str, _filter: &str) -> Result<Vec<Item>, Error> {
        let _ = (&self.token, &self.client);
        todo!("github query: issue search, filter passed through verbatim")
    }

    async fn open_prs(&self, _repo: &str, _branch_prefix: &str) -> Result<Vec<Pr>, Error> {
        todo!("github open_prs: list PRs, filter head branch by prefix")
    }

    async fn create_pr(&self, _req: &PrRequest) -> Result<Pr, Error> {
        todo!("github create_pr")
    }

    async fn comment(&self, _item_id: &str, _body: &str) -> Result<(), Error> {
        todo!("github comment")
    }

    async fn set_labels(&self, _item_id: &str, _labels: &[String]) -> Result<(), Error> {
        todo!("github set_labels")
    }
}
