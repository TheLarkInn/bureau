//! Azure DevOps forge: REST over reqwest with rustls. Never shells out
//! to `az` (DESIGN.md layer 7).

use async_trait::async_trait;

use super::{Error, Forge, Item, Pr, PrRequest};
use crate::process::Secret;

/// Azure DevOps API client. `source` uses `project/repo` form; `filter`
/// is a WIQL query passed through verbatim.
pub struct AdoForge {
    /// API root, e.g. `https://dev.azure.com/microsoft`; a local test
    /// server in tests.
    pub base_url: String,
    token: Secret,
    client: reqwest::Client,
}

impl AdoForge {
    /// A client against an ADO organization root.
    #[must_use]
    pub fn new(base_url: String, token: Secret) -> Self {
        Self {
            base_url,
            token,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Forge for AdoForge {
    async fn query(&self, _source: &str, _filter: &str) -> Result<Vec<Item>, Error> {
        let _ = (&self.token, &self.client);
        todo!("ado query: WIQL passed through verbatim, then hydrate items")
    }

    async fn open_prs(&self, _repo: &str, _branch_prefix: &str) -> Result<Vec<Pr>, Error> {
        todo!("ado open_prs")
    }

    async fn create_pr(&self, _req: &PrRequest) -> Result<Pr, Error> {
        todo!("ado create_pr")
    }

    async fn comment(&self, _item_id: &str, _body: &str) -> Result<(), Error> {
        todo!("ado comment")
    }

    async fn set_labels(&self, _item_id: &str, _labels: &[String]) -> Result<(), Error> {
        todo!("ado set_labels")
    }
}
