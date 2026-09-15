use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use bureau::forge::fake::FakeForge;
use bureau::forge::{Error, Forge, Item, Pr, PrRequest, PrStatus};
use tokio::sync::Notify;

pub struct Gate {
    forge: FakeForge,
    waiting: Notify,
    release: Notify,
}

impl Gate {
    pub fn new(item: Item) -> Arc<Self> {
        Arc::new(Self {
            forge: FakeForge::new(vec![item]),
            waiting: Notify::new(),
            release: Notify::new(),
        })
    }

    pub async fn wait<T>(&self, task: &mut tokio::task::JoinHandle<T>) {
        assert!(
            super::super::notify::first(self.waiting.notified(), task)
                .await
                .is_ok(),
            "reconciliation must reach forge observation before completing"
        );
    }

    pub async fn before_query<F: Future>(&self, operation: F) -> F::Output {
        super::super::notify::first(operation, self.waiting.notified())
            .await
            .expect("missing model auth must reject before forge observation")
    }

    pub fn release(&self) {
        self.release.notify_one();
    }
}

#[async_trait]
impl Forge for Gate {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        self.forge.query(source, filter).await
    }

    async fn open_prs(&self, repo: &str, prefix: &str) -> Result<Vec<Pr>, Error> {
        self.waiting.notify_one();
        self.release.notified().await;
        self.forge.open_prs(repo, prefix).await
    }

    async fn create_pr(&self, request: &PrRequest) -> Result<Pr, Error> {
        self.forge.create_pr(request).await
    }

    async fn pr_status(&self, repo: &str, number: u64) -> Result<PrStatus, Error> {
        self.forge.pr_status(repo, number).await
    }

    async fn comment(&self, item: &str, body: &str) -> Result<(), Error> {
        self.forge.comment(item, body).await
    }

    async fn set_labels(&self, item: &str, labels: &[String]) -> Result<(), Error> {
        self.forge.set_labels(item, labels).await
    }

    async fn update_labels(
        &self,
        item: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error> {
        self.forge.update_labels(item, add, remove).await
    }
}
