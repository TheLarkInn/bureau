//! Active run collection, drain, and second-signal cancellation.

use crate::cli::out;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use bureau::engine::RunOutcome;
use bureau::reconcile::Started;

async fn forward(mut signal: tokio::signal::unix::Signal, sender: tokio::sync::mpsc::Sender<()>) {
    while signal.recv().await.is_some() && sender.send(()).await.is_ok() {}
}

pub(in crate::cli) enum Until<T> {
    Complete(T),
    Signalled,
}

fn report(joined: Result<RunOutcome, tokio::task::JoinError>) -> Option<RunOutcome> {
    match joined {
        Ok(outcome) => {
            out::line(format_args!(
                "{} {} {}",
                outcome.run_id,
                bureau::runlog::outcome_name(outcome.outcome),
                outcome.message
            ));
            Some(outcome)
        }
        Err(error) => {
            out::error(format_args!("active run task failed: {error}"));
            None
        }
    }
}

fn cancel_all(runs_dir: &Path, run_ids: &[String]) {
    for run_id in run_ids {
        let path = bureau::runlog::run_dir(runs_dir, run_id).join("CANCEL");
        if let Err(error) = std::fs::write(path, "second shutdown signal cancelled this run") {
            out::error(format_args!("failed to cancel run {run_id}: {error}"));
        }
    }
}

fn release_all(owners: &[bureau::state::LeaseOwner]) {
    for owner in owners {
        if let Err(error) = owner.release() {
            out::error(format_args!(
                "forced shutdown lease release failed: {error}"
            ));
        }
    }
}

async fn finish_drain(
    result: Until<Vec<RunOutcome>>,
    joined: Pin<Box<impl Future<Output = Vec<RunOutcome>>>>,
    runs_dir: &Path,
    ids: &[String],
    aborts: &[tokio::task::AbortHandle],
    owners: &[bureau::state::LeaseOwner],
) -> Vec<RunOutcome> {
    match result {
        Until::Complete(outcomes) => outcomes,
        Until::Signalled => {
            cancel_all(runs_dir, ids);
            for abort in aborts {
                abort.abort();
            }
            let outcomes = joined.await;
            release_all(owners);
            outcomes
        }
    }
}

pub(in crate::cli) struct Signals {
    receiver: tokio::sync::mpsc::Receiver<()>,
}

impl Signals {
    pub(in crate::cli) fn new() -> std::io::Result<Self> {
        let interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        // One slot per signal source: a second signal (force-cancel) is
        // never dropped while a first one is still queued.
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        tokio::spawn(forward(interrupt, sender.clone()));
        tokio::spawn(forward(terminate, sender));
        Ok(Self { receiver })
    }

    pub(super) async fn recv(&mut self) {
        let _closed_or_signalled = self.receiver.recv().await;
    }
}

pub(in crate::cli) async fn until_signal<T, F>(
    mut future: Pin<&mut F>,
    signals: &mut Signals,
) -> Until<T>
where
    F: Future<Output = T> + ?Sized,
{
    std::future::poll_fn(|context| {
        if signals.receiver.poll_recv(context).is_ready() {
            return Poll::Ready(Until::Signalled);
        }
        future.as_mut().poll(context).map(Until::Complete)
    })
    .await
}

pub(super) async fn wait_or_signal(interval: Duration, signals: &mut Signals) -> bool {
    tokio::time::timeout(interval, signals.recv()).await.is_ok()
}

async fn join(runs: Vec<Started>) -> Vec<RunOutcome> {
    let mut outcomes = Vec::new();
    for run in runs {
        if let Some(outcome) = report(run.handle.await) {
            outcomes.push(outcome);
        }
    }
    outcomes
}

pub(super) struct Active {
    runs: Vec<Started>,
    runs_dir: PathBuf,
}

impl Active {
    pub(super) const fn new(runs_dir: PathBuf) -> Self {
        Self {
            runs: Vec::new(),
            runs_dir,
        }
    }

    pub(super) fn extend(&mut self, runs: Vec<Started>) {
        self.runs.extend(runs);
    }

    pub(super) fn contains(&self, run_id: &str) -> bool {
        self.runs.iter().any(|run| run.run_id == run_id)
    }

    pub(super) fn ids(&self) -> Vec<String> {
        self.runs.iter().map(|run| run.run_id.clone()).collect()
    }

    pub(super) async fn reap(&mut self) {
        while let Some(index) = self.runs.iter().position(|run| run.handle.is_finished()) {
            let started = self.runs.swap_remove(index);
            report(started.handle.await);
        }
    }

    pub(super) async fn drain(self, signals: &mut Signals) -> Vec<RunOutcome> {
        let ids: Vec<_> = self.runs.iter().map(|run| run.run_id.clone()).collect();
        let aborts: Vec<_> = self
            .runs
            .iter()
            .map(|run| run.handle.abort_handle())
            .collect();
        let owners: Vec<_> = self
            .runs
            .iter()
            .filter_map(|run| run.owner.clone())
            .collect();
        let mut joined = Box::pin(join(self.runs));
        let result = until_signal(joined.as_mut(), signals).await;
        finish_drain(result, joined, &self.runs_dir, &ids, &aborts, &owners).await
    }
}
