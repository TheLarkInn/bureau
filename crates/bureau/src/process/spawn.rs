//! Layer 0: spawning a subprocess under the process contract.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};

use super::scrub::ScrubWriter;
use super::secret::Secret;

/// Where captured output goes besides the result buffers: the run log.
pub type SharedLog = Arc<Mutex<Box<dyn Write + Send>>>;

/// Wraps a writer for use as a [`SpawnRequest::log`] sink.
#[must_use]
pub fn shared_log<W>(writer: W) -> SharedLog
where
    W: Write + Send + 'static,
{
    Arc::new(Mutex::new(Box::new(writer)))
}

/// Everything needed to run one subprocess (DESIGN.md layer 0).
pub struct SpawnRequest {
    /// Program and arguments; `argv[0]` is the program.
    pub argv: Vec<String>,
    /// Working directory — always a worktree path, never the daemon's cwd.
    pub dir: PathBuf,
    /// The COMPLETE child environment; nothing is inherited from the daemon.
    pub env: BTreeMap<String, String>,
    /// Bytes written to the child's stdin, then closed.
    pub stdin: Vec<u8>,
    /// Hard limit; on expiry the child's whole process group is killed.
    pub timeout: Duration,
    /// Values scrubbed from all captured output.
    pub secrets: Vec<Secret>,
    /// Optional sink receiving scrubbed output as it arrives.
    pub log: Option<SharedLog>,
    /// Optional durable cancellation marker.
    pub cancel: Option<PathBuf>,
}

/// How a spawned process ended. These four outcomes are genuinely
/// distinct; a timeout is never collapsed into an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnOutcome {
    /// Ran to completion; see `exit_code`.
    Exited,
    /// Hard-killed (the whole process group) after `timeout`.
    Timeout,
    /// Killed externally by a signal.
    Signaled,
    /// Never started.
    SpawnFailed,
}

/// The captured result of one subprocess. A struct with an enum
/// discriminant, so it serializes straight into the run log.
#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnResult {
    /// How the process ended.
    pub outcome: SpawnOutcome,
    /// `Some` only when `outcome == Exited`.
    pub exit_code: Option<i32>,
    /// Scrubbed stdout.
    pub stdout: Vec<u8>,
    /// Scrubbed stderr.
    pub stderr: Vec<u8>,
    /// Wall time from spawn to reaping.
    pub duration: Duration,
    /// Spawn or wait error detail, when relevant.
    pub error: Option<String>,
}

impl SpawnResult {
    fn failed(error: String, started: Instant) -> Self {
        Self {
            outcome: SpawnOutcome::SpawnFailed,
            exit_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: started.elapsed(),
            error: Some(error),
        }
    }
}

fn build_command(req: &SpawnRequest) -> Result<Command, String> {
    let Some(program) = req.argv.first() else {
        return Err("argv is empty".to_owned());
    };
    let mut command = std::process::Command::new(program);
    command
        .args(&req.argv[1..])
        .current_dir(&req.dir)
        .env_clear()
        .envs(&req.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A new process group per child (pgid == child pid) so a timeout can
    // kill the whole tree, backgrounded grandchildren included. Both
    // `process_group` and nix's `killpg` are safe APIs; no `unsafe` here.
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    Ok(Command::from(command))
}

fn spawn_child(req: &SpawnRequest) -> Result<Child, String> {
    build_command(req)?.spawn().map_err(|e| e.to_string())
}

/// Runs a subprocess to completion under the contract.
#[must_use]
pub async fn spawn(req: SpawnRequest) -> SpawnResult {
    let started = Instant::now();
    match spawn_child(&req) {
        Ok(child) => run_child(req, child, started).await,
        Err(error) => SpawnResult::failed(error, started),
    }
}

async fn run_child(req: SpawnRequest, mut child: Child, started: Instant) -> SpawnResult {
    let stdin = write_task(child.stdin.take(), req.stdin);
    let out = drain_task(child.stdout.take(), req.secrets.clone(), req.log.clone());
    let err = drain_task(child.stderr.take(), req.secrets, req.log);
    let (outcome, exit_code, error) =
        super::wait::wait_child(&mut child, req.timeout, req.cancel.as_deref()).await;
    // A timeout kills the process group, closing the pipes: the drains see
    // EOF and the writer gets EPIPE, so joining all three never hangs.
    let (_, stdout, stderr) = tokio::join!(stdin, out, err);
    SpawnResult {
        outcome,
        exit_code,
        stdout: stdout.unwrap_or_default(),
        stderr: stderr.unwrap_or_default(),
        duration: started.elapsed(),
        error,
    }
}

/// Writes `bytes` to the child's stdin in its own task, so a child that
/// never reads stdin can't block the drains or delay the arming of the
/// timeout. Dropping the pipe closes the child's stdin.
fn write_task(pipe: Option<ChildStdin>, bytes: Vec<u8>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Some(mut pipe) = pipe else {
            return;
        };
        let _ = pipe.write_all(&bytes).await;
    })
}

type BoxedStream = Pin<Box<dyn AsyncRead + Send>>;

fn stream<R>(stream: Option<R>) -> BoxedStream
where
    R: AsyncRead + Send + Unpin + 'static,
{
    stream.map_or_else(
        || Box::pin(tokio::io::empty()) as BoxedStream,
        |r| Box::pin(r) as BoxedStream,
    )
}

fn drain_task(
    reader: Option<impl AsyncRead + Send + Unpin + 'static>,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> tokio::task::JoinHandle<Vec<u8>> {
    tokio::spawn(drain(stream(reader), secrets, log))
}

/// Reads a stream to EOF, scrubbing each chunk as it arrives and
/// forwarding the scrubbed bytes to the run-log sink immediately.
async fn drain(reader: BoxedStream, secrets: Vec<Secret>, log: Option<SharedLog>) -> Vec<u8> {
    let mut reader = reader;
    let mut scrubber = ScrubWriter::new(Vec::new(), &secrets);
    let mut forwarded = 0;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break, // a read error still leaves partial output usable
            Ok(n) => {
                let _ = scrubber.write_all(&chunk[..n]);
                forward(log.as_ref(), scrubber.get_ref(), &mut forwarded);
            }
        }
    }
    let captured = scrubber.finish().unwrap_or_default();
    forward(log.as_ref(), &captured, &mut forwarded);
    captured
}

fn forward(log: Option<&SharedLog>, buf: &[u8], forwarded: &mut usize) {
    if buf.len() <= *forwarded {
        return;
    }
    if let Some(sink) = log {
        if let Ok(mut w) = sink.lock() {
            let _ = w.write_all(&buf[*forwarded..]);
        }
    }
    *forwarded = buf.len();
}
