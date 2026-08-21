//! Layer 7: forges (DESIGN.md section 7). Implement the [`Forge`] trait
//! to add one; use [`fake::FakeForge`] in tests.
//!
//! A forge is the GitHub or ADO API integration. The runner consumes it
//! through this interface and never reimplements what the forge owns
//! (work items, PRs, labels, review, identity — DESIGN.md section 1).

pub mod ado;
pub mod fake;
pub mod github;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::contract::Trust;

/// Which forge hosts a repo or work source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    /// Azure DevOps.
    Ado,
    /// GitHub.
    Github,
}

/// A work item from the work source (bug, issue, task — the forge owns
/// taxonomy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// The forge's id for the item (issue number, work item id).
    pub external_id: String,
    /// One-line summary.
    pub title: String,
    /// Full description.
    pub body: String,
    /// Human-facing URL.
    pub url: String,
    /// Current labels / tags.
    pub labels: Vec<String>,
    /// Provenance grade of `body` (DESIGN.md section 9).
    pub trust: Trust,
}

impl Item {
    /// Stable content hash for dedup (DESIGN.md layer 5): an identical
    /// proposal already open or previously rejected must exit `NoWork`.
    ///
    /// Uses std's `DefaultHasher`: stable within one binary version, which
    /// is all the dedup window needs.
    #[must_use]
    pub fn content_hash(&self) -> String {
        use std::hash::{Hash, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.title.hash(&mut hasher);
        self.body.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

impl From<&Item> for crate::contract::WorkItem {
    /// Projects a work item onto the step contract. `trust` is not
    /// carried: `StepRequest::trust` already holds the request's floor.
    ///
    /// Destructured on purpose, and one-way on purpose. Destructuring
    /// makes a new [`Item`] field a compile error here, so carrying or
    /// dropping it is an explicit decision; the reverse conversion is
    /// deliberately absent, because a step's copy must never become a
    /// source of truth for the forge (DESIGN.md section 3).
    fn from(item: &Item) -> Self {
        let Item {
            external_id,
            title,
            body,
            url,
            labels,
            trust: _,
        } = item;
        Self {
            external_id: external_id.clone(),
            title: title.clone(),
            body: body.clone(),
            url: url.clone(),
            labels: labels.clone(),
        }
    }
}

/// An open pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pr {
    /// Forge-assigned number.
    pub number: u64,
    /// The repo it targets, in the forge's `owner/name` form.
    pub repo: String,
    /// Head branch.
    pub branch: String,
    /// Title.
    pub title: String,
    /// Human-facing URL.
    pub url: String,
    /// The work item it closes, when linked.
    pub item_id: Option<String>,
}

/// A pull request to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRequest {
    /// Target repo, in the forge's `owner/name` form.
    pub repo: String,
    /// Branch to merge (carries the assignment's `branch_prefix`).
    pub branch: String,
    /// Branch to merge into.
    pub base: String,
    /// Title.
    pub title: String,
    /// Description.
    pub body: String,
    /// The work item this PR closes, when known.
    pub item_id: Option<String>,
}

/// Current forge-owned pull-request state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrStatus {
    /// Review is still open.
    Open,
    /// Pull request closed without merge.
    Closed,
    /// Pull request merged, with an exact commit when the forge reports it.
    Merged {
        /// Merge commit or completed source commit.
        commit: Option<String>,
    },
}

/// A forge operation failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// HTTP transport failure.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// The forge rejected the call.
    #[error("forge API error (status {status}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Body or status text.
        message: String,
    },
    /// The response did not match the expected shape.
    #[error("unexpected forge response: {0}")]
    Parse(String),
}

/// The forge interface. `query` passes `filter` through verbatim — the
/// runner never parses it (WIQL for ADO, search syntax for GitHub).
#[async_trait]
pub trait Forge: Send + Sync {
    /// Work items matching the forge-native `filter` at `source`.
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error>;

    /// Open PRs in `repo` whose branch starts with `branch_prefix`.
    async fn open_prs(&self, repo: &str, branch_prefix: &str) -> Result<Vec<Pr>, Error>;

    /// Opens a pull request.
    async fn create_pr(&self, req: &PrRequest) -> Result<Pr, Error>;

    /// Current state of one pull request.
    async fn pr_status(&self, repo: &str, number: u64) -> Result<PrStatus, Error>;

    /// Comments on a work item.
    async fn comment(&self, item_id: &str, body: &str) -> Result<(), Error>;

    /// Replaces a work item's labels.
    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<(), Error>;

    /// Adds and removes named labels without changing unrelated labels.
    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error>;
}
