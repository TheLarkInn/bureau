//! Preserve a buffered shutdown reply when normal process exit wins the poll race.

use std::future::Future;
use std::time::Duration;

use tokio::process::{ChildStdin, ChildStdout};

use crate::process::{Duplex, DuplexOwner, SpawnOutcome, SpawnRequest, SpawnResult, start_duplex};

use super::race::{self, First};

pub(super) fn clean(result: &SpawnResult) -> bool {
    result.outcome == SpawnOutcome::Exited && result.exit_code == Some(0) && result.error.is_none()
}

async fn drain<I: Future>(result: SpawnResult, future: I) -> (SpawnResult, Option<I::Output>) {
    let output = if clean(&result) {
        tokio::time::timeout(Duration::from_secs(2), future)
            .await
            .ok()
    } else {
        None
    };
    (result, output)
}

async fn finish<I: Future>(
    outcome: First<I::Output, SpawnResult>,
    owner: DuplexOwner,
    future: I,
) -> (SpawnResult, Option<I::Output>) {
    match outcome {
        First::Left(output) => {
            drop(future);
            (owner.finish().await, Some(output))
        }
        First::Right(result) => drain(result, future).await,
    }
}

async fn interaction<F, I, T>(process: Duplex, interact: F) -> (SpawnResult, Option<T>)
where
    F: FnOnce(ChildStdin, ChildStdout) -> I,
    I: Future<Output = T>,
{
    let Duplex {
        stdin,
        stdout,
        mut owner,
    } = process;
    let mut future = Box::pin(interact(stdin, stdout));
    let outcome = race::first(&mut future, owner.wait()).await;
    finish(outcome, owner, future).await
}

pub(super) async fn run<F, I, T>(request: SpawnRequest, interact: F) -> (SpawnResult, Option<T>)
where
    F: FnOnce(ChildStdin, ChildStdout) -> I,
    I: Future<Output = T>,
{
    match start_duplex(request) {
        Ok(process) => interaction(process, interact).await,
        Err(result) => (result, None),
    }
}
