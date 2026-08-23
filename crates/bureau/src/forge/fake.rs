//! An in-memory forge for offline tests — the same role the `fake`
//! adapter plays for agent CLIs (DESIGN.md layer 1 discipline).
//!
//! `query` returns every stored item regardless of `filter`: filtering is
//! the real forge's job, and tests using the fake pass the whole world in
//! at construction.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use async_trait::async_trait;

use super::{Dependency, Error, Forge, Item, LabelForge, Pr, PrRequest, PrStatus};

/// Lock that survives a poisoned mutex: a panicking test must not cascade
/// into misleading secondary failures.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn updated_labels(
    items: &mut [Item],
    item_id: &str,
    add: &[String],
    remove: &[String],
) -> Vec<String> {
    items
        .iter_mut()
        .find(|item| item.external_id == item_id)
        .map_or_else(Vec::new, |item| {
            item.labels.retain(|label| !remove.contains(label));
            for label in add {
                if !item.labels.contains(label) {
                    item.labels.push(label.clone());
                }
            }
            item.labels.clone()
        })
}

/// An in-memory [`Forge`] driven entirely by construction-time state.
#[derive(Debug, Default)]
pub struct FakeForge {
    items: Mutex<Vec<Item>>,
    prs: Mutex<Vec<Pr>>,
    comments: Mutex<Vec<(String, String)>>,
    labels: Mutex<BTreeMap<String, Vec<String>>>,
    dependencies: Mutex<BTreeMap<String, Vec<Dependency>>>,
    label_failure: Mutex<Option<String>>,
    rate_limit_once: AtomicBool,
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
        lock(&self.labels)
            .get(item_id)
            .cloned()
            .or_else(|| {
                lock(&self.items)
                    .iter()
                    .find(|item| item.external_id == item_id)
                    .map(|item| item.labels.clone())
            })
            .unwrap_or_default()
    }

    /// Removes an item, as the forge would when it is closed.
    pub fn remove_item(&self, external_id: &str) {
        lock(&self.items).retain(|i| i.external_id != external_id);
    }

    /// Replaces the blocking dependencies returned for one item.
    pub fn set_dependencies(&self, item_id: &str, dependencies: Vec<Dependency>) {
        lock(&self.dependencies).insert(item_id.to_owned(), dependencies);
    }

    /// Makes subsequent label updates fail with `message`.
    pub fn fail_label_updates(&self, message: &str) {
        *lock(&self.label_failure) = Some(message.to_owned());
    }

    /// Makes the next label update report a rate limit.
    pub fn rate_limit_next_label_update(&self) {
        self.rate_limit_once.store(true, Ordering::Relaxed);
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

    async fn pr_status(&self, _: &str, _: u64) -> Result<PrStatus, Error> {
        Ok(PrStatus::Open)
    }

    async fn comment(&self, item_id: &str, body: &str) -> Result<(), Error> {
        lock(&self.comments).push((item_id.to_owned(), body.to_owned()));
        Ok(())
    }

    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<(), Error> {
        if let Some(item) = lock(&self.items)
            .iter_mut()
            .find(|item| item.external_id == item_id)
        {
            item.labels = labels.to_vec();
        }
        lock(&self.labels).insert(item_id.to_owned(), labels.to_vec());
        Ok(())
    }

    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error> {
        if self.rate_limit_once.swap(false, Ordering::Relaxed) {
            return Err(Error::RateLimited {
                retry_after_secs: Some(60),
                message: "fake rate limit".to_owned(),
            });
        }
        let failure = lock(&self.label_failure).clone();
        if let Some(message) = failure {
            return Err(Error::Parse(message));
        }
        let labels = {
            let mut items = lock(&self.items);
            updated_labels(&mut items, item_id, add, remove)
        };
        lock(&self.labels).insert(item_id.to_owned(), labels);
        Ok(())
    }
}

#[async_trait]
impl LabelForge for FakeForge {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        <Self as Forge>::query(self, source, filter).await
    }

    async fn item(&self, item_id: &str) -> Result<Item, Error> {
        lock(&self.items)
            .iter()
            .find(|item| item.external_id == item_id)
            .cloned()
            .ok_or_else(|| Error::Parse(format!("work item `{item_id}` not found")))
    }

    async fn blocking_dependencies(&self, item_id: &str) -> Result<Vec<Dependency>, Error> {
        Ok(lock(&self.dependencies)
            .get(item_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error> {
        <Self as Forge>::update_labels(self, item_id, add, remove).await
    }
}
