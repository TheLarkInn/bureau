//! Waiting, timeout, cancellation, and process-group termination.

use std::process::ExitStatus;
use std::time::{Duration, Instant};

use tokio::process::Child;

use super::SpawnOutcome;

pub(super) async fn wait_child(
    child: &mut Child,
    timeout: Duration,
    cancel: Option<&std::path::Path>,
) -> (SpawnOutcome, Option<i32>, Option<String>) {
    let event = wait_event(child, timeout, cancel).await;
    complete_wait(child, event).await
}

async fn complete_wait(
    child: &mut Child,
    event: Wait,
) -> (SpawnOutcome, Option<i32>, Option<String>) {
    match event {
        Wait::Exited(Ok(status)) => classify(status),
        Wait::Exited(Err(error)) => (
            SpawnOutcome::Signaled,
            None,
            Some(format!("wait failed: {error}")),
        ),
        Wait::Cancelled => stop(child, SpawnOutcome::Signaled, Some("cancelled".to_owned())).await,
        Wait::Timeout => stop(child, SpawnOutcome::Timeout, None).await,
    }
}

enum Wait {
    Exited(std::io::Result<ExitStatus>),
    Cancelled,
    Timeout,
}

async fn wait_event(
    child: &mut Child,
    timeout: Duration,
    cancel: Option<&std::path::Path>,
) -> Wait {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(event) = poll(child, cancel, deadline) {
            return event;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn poll(child: &mut Child, cancel: Option<&std::path::Path>, deadline: Instant) -> Option<Wait> {
    match child.try_wait() {
        Ok(Some(status)) => return Some(Wait::Exited(Ok(status))),
        Err(error) => return Some(Wait::Exited(Err(error))),
        Ok(None) => {}
    }
    if cancel.is_some_and(std::path::Path::exists) {
        return Some(Wait::Cancelled);
    }
    (Instant::now() >= deadline).then_some(Wait::Timeout)
}

async fn stop(
    child: &mut Child,
    outcome: SpawnOutcome,
    error: Option<String>,
) -> (SpawnOutcome, Option<i32>, Option<String>) {
    kill_group(child);
    match child.wait().await {
        Ok(_) => (outcome, None, error),
        Err(wait_error) => (
            SpawnOutcome::Signaled,
            None,
            Some(format!("wait after termination failed: {wait_error}")),
        ),
    }
}

fn classify(status: ExitStatus) -> (SpawnOutcome, Option<i32>, Option<String>) {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => (SpawnOutcome::Exited, Some(code), None),
        (None, signal) => (
            SpawnOutcome::Signaled,
            None,
            signal.map(|signal| format!("signal {signal}")),
        ),
    }
}

fn kill_group(child: &Child) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let Some(id) = child.id() else {
        return;
    };
    let Ok(pgid) = i32::try_from(id) else {
        return;
    };
    let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL);
}
