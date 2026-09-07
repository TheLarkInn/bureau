//! Owned protocol pipes with the same process-tree supervision as buffered spawn.

use std::future::Future;
use std::path::PathBuf;
use std::task::Poll;
use std::time::Instant;

use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::task::JoinHandle;

use super::spawn::{DRAIN_TIMEOUT, drain_task, monotonic_now, spawn_child};
use super::wait::{KillOnDrop, Wait, shutdown, wait_child_until};
use super::{SpawnOutcome, SpawnRequest, SpawnResult};

fn append_error(result: &mut SpawnResult, error: &str) {
    match &mut result.error {
        Some(existing) => {
            existing.push_str("; ");
            existing.push_str(error);
        }
        slot @ None => *slot = Some(error.to_owned()),
    }
}

fn result(started: Instant, outcome: SpawnOutcome, error: Option<String>) -> SpawnResult {
    SpawnResult {
        outcome,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        duration: started.elapsed(),
        error,
    }
}

/// Owns supervision independently of the protocol pipes.
///
/// Race [`Self::wait`] against the entire interaction, including initialization.
/// There is no detached supervisor: dropping this owner kills the process tree
/// and aborts its stderr drain even when the interaction has not completed.
pub struct DuplexOwner {
    child: Child,
    kill_on_drop: KillOnDrop,
    stderr: Option<JoinHandle<Vec<u8>>>,
    started: Instant,
    deadline: Instant,
    cancel: Option<PathBuf>,
    event: Option<Wait>,
}

impl DuplexOwner {
    /// Waits for actual process exit, cancellation, or the original spawn deadline.
    ///
    /// Cancellation-safe: a losing `select!` branch may be dropped, then resumed
    /// by [`Self::finish`]. Protocol stdout belongs to the caller, not the result.
    pub async fn wait(&mut self) -> SpawnResult {
        let (outcome, exit_code, error) = wait_child_until(
            &mut self.child,
            self.deadline,
            self.cancel.as_deref(),
            &mut self.event,
        )
        .await;
        self.kill_on_drop.finish();
        let mut result = result(self.started, outcome, error);
        result.exit_code = exit_code;
        match self.drain_stderr().await {
            Ok(stderr) => result.stderr = stderr,
            Err(error) => append_error(&mut result, &error),
        }
        result.duration = self.started.elapsed();
        result
    }

    /// After closing protocol pipes, allows one second for clean exit.
    ///
    /// A server ignoring EOF is killed as a tree and reaped, reporting `Signaled`
    /// rather than inventing an exit code. The original timeout and cancellation
    /// remain active throughout shutdown.
    pub async fn finish(mut self) -> SpawnResult {
        if let Ok(result) = tokio::time::timeout(DRAIN_TIMEOUT, self.wait()).await {
            return result;
        }
        shutdown(&mut self.child, &mut self.event);
        self.kill_on_drop.kill();
        self.wait().await
    }

    async fn drain_stderr(&mut self) -> Result<Vec<u8>, String> {
        let Some(task) = self.stderr.as_mut() else {
            return Err("stderr capture has already been consumed".to_owned());
        };
        let drained = tokio::time::timeout(DRAIN_TIMEOUT, &mut *task).await;
        match drained {
            Ok(result) => {
                self.stderr = None;
                result.map_err(|error| format!("stderr capture task failed: {error}"))
            }
            Err(error) => {
                task.abort();
                Err(format!("stderr capture timed out: {error}"))
            }
        }
    }
}

impl Drop for DuplexOwner {
    fn drop(&mut self) {
        self.kill_on_drop.kill();
        if let Some(task) = &self.stderr {
            task.abort();
        }
    }
}

/// Protocol pipes and their process owner. Keep the owner in the interaction future.
pub struct Duplex {
    /// Raw protocol input; close it before finishing the owner.
    pub stdin: ChildStdin,
    /// Raw protocol output, intentionally neither captured nor scrubbed.
    pub stdout: ChildStdout,
    /// Process lifetime, deadline, cancellation, and scrubbed stderr capture.
    pub owner: DuplexOwner,
}

fn missing_pipe(started: Instant) -> SpawnResult {
    result(
        started,
        SpawnOutcome::Signaled,
        Some("spawned process is missing a protocol pipe".to_owned()),
    )
}

fn connect(
    req: SpawnRequest,
    mut child: Child,
    token: &str,
    started: Instant,
) -> Result<Duplex, SpawnResult> {
    let kill_on_drop = KillOnDrop::new(&child, token);
    let missing = || missing_pipe(started);
    let stdin = child.stdin.take().ok_or_else(missing)?;
    let stdout = child.stdout.take().ok_or_else(missing)?;
    let stderr = child.stderr.take().ok_or_else(missing)?;
    let owner = DuplexOwner {
        child,
        kill_on_drop,
        stderr: Some(drain_task(Some(stderr), req.secrets, req.log)),
        started,
        deadline: started + req.timeout,
        cancel: req.cancel,
        event: None,
    };
    Ok(Duplex {
        stdin,
        stdout,
        owner,
    })
}

/// Starts an interactive subprocess with detached, owned protocol pipes.
///
/// Uses the buffered spawn environment, work directory, process-group and
/// descendant isolation. Stderr is scrubbed and streamed to `req.log`; callers
/// must scrub any protocol content they choose to persist themselves.
///
/// # Errors
///
/// Returns a process result for invalid requests or spawn failure. `req.stdin`
/// must be empty: the protocol caller owns all writes to the input pipe.
pub fn start_duplex(req: SpawnRequest) -> Result<Duplex, SpawnResult> {
    let started = monotonic_now();
    if !req.stdin.is_empty() {
        return Err(result(
            started,
            SpawnOutcome::SpawnFailed,
            Some("duplex stdin must be empty; write to the protocol pipe".to_owned()),
        ));
    }
    let (child, token) = spawn_child(&req)
        .map_err(|error| result(started, SpawnOutcome::SpawnFailed, Some(error)))?;
    connect(req, child, &token, started)
}

async fn interact<I: Future>(
    owner: &mut DuplexOwner,
    interaction: I,
) -> Result<I::Output, SpawnResult> {
    let mut waiting = std::pin::pin!(owner.wait());
    let mut interaction = std::pin::pin!(interaction);
    std::future::poll_fn(|context| match waiting.as_mut().poll(context) {
        Poll::Ready(result) => Poll::Ready(Err(result)),
        Poll::Pending => interaction.as_mut().poll(context).map(Ok),
    })
    .await
}

async fn supervise<F, I, T>(process: Duplex, interaction: F) -> (SpawnResult, Option<T>)
where
    F: FnOnce(ChildStdin, ChildStdout) -> I,
    I: Future<Output = T>,
{
    let Duplex {
        stdin,
        stdout,
        mut owner,
    } = process;
    let output = interact(&mut owner, interaction(stdin, stdout)).await;
    match output {
        Ok(output) => (owner.finish().await, Some(output)),
        Err(result) => (result, None),
    }
}

/// Runs an entire protocol interaction under process supervision.
///
/// The interaction owns both pipes and is dropped before bounded shutdown.
/// Its output is `None` when process supervision finishes first. The process
/// result always describes actual exit or termination, never protocol success.
pub async fn duplex<F, I, T>(req: SpawnRequest, interaction: F) -> (SpawnResult, Option<T>)
where
    F: FnOnce(ChildStdin, ChildStdout) -> I,
    I: Future<Output = T>,
{
    match start_duplex(req) {
        Ok(process) => supervise(process, interaction).await,
        Err(result) => (result, None),
    }
}
