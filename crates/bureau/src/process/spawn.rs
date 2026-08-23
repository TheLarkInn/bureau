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

use super::SpawnOutcome;
use super::scrub::ScrubWriter;
use super::secret::Secret;

const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const UNSHARE_ARGS: [&str; 7] = [
    "--user",
    "--map-root-user",
    "--pid",
    "--fork",
    "--kill-child=SIGKILL",
    "--mount-proc",
    "--",
];
const INIT_SCRIPT: &str = r#""$@"; exit $?"#;

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

fn executable(path: PathBuf, program: &str) -> Result<PathBuf, String> {
    path.is_file()
        .then_some(path)
        .ok_or_else(|| format!("program `{program}` was not found"))
}

fn resolve_program(program: &str, req: &SpawnRequest) -> Result<PathBuf, String> {
    let path = std::path::Path::new(program);
    if path.components().count() > 1 {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            req.dir.join(path)
        };
        return executable(path, program);
    }
    // The process environment boundary: PATH fallback when the request's
    // complete environment omits it.
    let env_var_os = std::env::var_os;
    let search = req
        .env
        .get("PATH")
        .map(std::ffi::OsString::from)
        .or_else(|| env_var_os("PATH"))
        .unwrap_or_default();
    std::env::split_paths(&search)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("program `{program}` was not found"))
}

fn build_command(req: &SpawnRequest, token: &str) -> Result<Command, String> {
    let Some(program) = req.argv.first() else {
        return Err("argv is empty".to_owned());
    };
    let program = resolve_program(program, req)?;
    let mut command = std::process::Command::new("unshare");
    command
        .args(UNSHARE_ARGS)
        .arg("/bin/sh")
        .arg("-c")
        .arg(INIT_SCRIPT)
        .arg("bureau-init")
        .arg(program)
        .args(&req.argv[1..])
        .current_dir(&req.dir)
        .env_clear()
        .envs(&req.env)
        .env(super::wait::TOKEN_VAR, token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A process group supplements the unescapable PID namespace.
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    Ok(Command::from(command))
}

fn spawn_child(req: &SpawnRequest) -> Result<(Child, String), String> {
    let token = crate::identity::random_hex().map_err(|error| error.to_string())?;
    let child = build_command(req, &token)
        .and_then(|mut command| command.spawn().map_err(|error| error.to_string()))?;
    Ok((child, token))
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

/// How much of `available` may be handed to the sink now.
///
/// `last` is EOF for this stream: nothing can finish a trailing character, so
/// it goes out and decodes lossily. Before then an unfinished character waits
/// for the next read, so the sink never decodes half of one.
const fn emittable(available: &[u8], last: bool) -> usize {
    if last {
        available.len()
    } else {
        super::utf8::complete_prefix(available)
    }
}

/// Forwards newly scrubbed bytes to the run-log sink.
fn forward(log: Option<&SharedLog>, buf: &[u8], forwarded: &mut usize, last: bool) {
    let available = &buf[(*forwarded).min(buf.len())..];
    let take = emittable(available, last);
    if take == 0 {
        return;
    }
    if let Some(sink) = log {
        if let Ok(mut w) = sink.lock() {
            let _ = w.write_all(&available[..take]);
        }
    }
    *forwarded += take;
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
                forward(log.as_ref(), scrubber.get_ref(), &mut forwarded, false);
            }
        }
    }
    let captured = scrubber.finish().unwrap_or_default();
    forward(log.as_ref(), &captured, &mut forwarded, true);
    captured
}

fn drain_task(
    reader: Option<impl AsyncRead + Send + Unpin + 'static>,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> tokio::task::JoinHandle<Vec<u8>> {
    tokio::spawn(drain(stream(reader), secrets, log))
}

async fn finish_drains(
    mut stdin: tokio::task::JoinHandle<()>,
    mut out: tokio::task::JoinHandle<Vec<u8>>,
    mut err: tokio::task::JoinHandle<Vec<u8>>,
) -> (Vec<u8>, Vec<u8>) {
    let drained = tokio::time::timeout(DRAIN_TIMEOUT, async {
        tokio::join!(&mut stdin, &mut out, &mut err)
    })
    .await;
    if let Ok((_, stdout, stderr)) = drained {
        return (stdout.unwrap_or_default(), stderr.unwrap_or_default());
    }
    stdin.abort();
    out.abort();
    err.abort();
    (Vec::new(), Vec::new())
}

async fn run_child(
    req: SpawnRequest,
    mut child: Child,
    token: &str,
    started: Instant,
) -> SpawnResult {
    let mut kill_on_drop = super::wait::KillOnDrop::new(&child, token);
    let stdin = write_task(child.stdin.take(), req.stdin);
    let out = drain_task(child.stdout.take(), req.secrets.clone(), req.log.clone());
    let err = drain_task(child.stderr.take(), req.secrets, req.log);
    let (outcome, exit_code, error) =
        super::wait::wait_child(&mut child, req.timeout, req.cancel.as_deref()).await;
    kill_on_drop.finish();
    // A timeout kills the process group, closing the pipes: the drains see
    // EOF and the writer gets EPIPE, so joining all three never hangs.
    let (stdout, stderr) = finish_drains(stdin, out, err).await;
    SpawnResult {
        outcome,
        exit_code,
        stdout,
        stderr,
        duration: started.elapsed(),
        error,
    }
}

/// The process clock boundary: the single place wall time is read.
fn monotonic_now() -> Instant {
    let now = std::time::Instant::now;
    now()
}

/// Runs a subprocess to completion under the contract.
#[must_use]
pub async fn spawn(req: SpawnRequest) -> SpawnResult {
    let started = monotonic_now();
    match spawn_child(&req) {
        Ok((child, token)) => run_child(req, child, &token, started).await,
        Err(error) => SpawnResult {
            outcome: SpawnOutcome::SpawnFailed,
            exit_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: started.elapsed(),
            error: Some(error),
        },
    }
}
