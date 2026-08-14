//! An in-memory forge for offline tests — the same role the `fake`
//! adapter plays for agent CLIs (DESIGN.md layer 1 discipline).
//!
//! `query` returns every stored item regardless of `filter`: filtering is
//! the real forge's job, and tests using the fake pass the whole world in
//! at construction.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use async_trait::async_trait;

use super::{Error, Forge, Item, Pr, PrRequest};

/// Lock that survives a poisoned mutex: a panicking test must not cascade
/// into misleading secondary failures.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// An in-memory [`Forge`] driven entirely by construction-time state.
#[derive(Debug, Default)]
pub struct FakeForge {
    items: Mutex<Vec<Item>>,
    prs: Mutex<Vec<Pr>>,
    comments: Mutex<Vec<(String, String)>>,
    labels: Mutex<BTreeMap<String, Vec<String>>>,
    next_pr_number: AtomicU64,
}

impl FakeForge {
    /// A forge containing exactly `items` and no PRs.
    #[must_use]
    pub fn new(items: Vec<Item>) -> Self {
        Self {
            items: Mutex::new(items),
            next_pr_number: AtomicU64::new(1),
            ..Self::default()
        }
    }

    /// Every comment recorded, as `(item_id, body)` pairs.
    #[must_use]
    pub fn comments(&self) -> Vec<(String, String)> {
        lock(&self.comments).clone()
    }

    /// The labels currently set on an item.
    #[must_use]
    pub fn labels_of(&self, item_id: &str) -> Vec<String> {
        lock(&self.labels).get(item_id).cloned().unwrap_or_default()
    }

    /// Removes an item, as the forge would when it is closed.
    pub fn remove_item(&self, external_id: &str) {
        lock(&self.items).retain(|i| i.external_id != external_id);
    }
}

#[async_trait]
impl Forge for FakeForge {
    async fn query(&self, _source: &str, _filter: &str) -> Result<Vec<Item>, Error> {
        Ok(lock(&self.items).clone())
    }

    async fn open_prs(&self, repo: &str, branch_prefix: &str) -> Result<Vec<Pr>, Error> {
        Ok(lock(&self.prs)
            .iter()
            .filter(|pr| pr.repo == repo && pr.branch.starts_with(branch_prefix))
            .cloned()
            .collect())
    }

    async fn create_pr(&self, req: &PrRequest) -> Result<Pr, Error> {
        let pr = Pr {
            number: self.next_pr_number.fetch_add(1, Ordering::Relaxed),
            repo: req.repo.clone(),
            branch: req.branch.clone(),
            title: req.title.clone(),
            url: format!("fake://pr/{}", self.next_pr_number.load(Ordering::Relaxed)),
            item_id: req.item_id.clone(),
        };
        lock(&self.prs).push(pr.clone());
        Ok(pr)
    }

    async fn comment(&self, item_id: &str, body: &str) -> Result<(), Error> {
        lock(&self.comments).push((item_id.to_owned(), body.to_owned()));
        Ok(())
    }

    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<(), Error> {
        lock(&self.labels).insert(item_id.to_owned(), labels.to_vec());
        Ok(())
    }
}
