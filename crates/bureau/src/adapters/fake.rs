//! The `fake` adapter: replays a recorded transcript from a fixture file
//! (DESIGN.md layer 1). `record` captures a real subprocess through the
//! layer-0 contract and writes such a fixture.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::StepDef;
use crate::contract::{SCHEMA_VERSION, StepRequest, StepResult};
use crate::process::{Secret, SharedLog, SpawnRequest, SpawnResult, spawn};

/// Which stream a chunk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// One output chunk in a recorded transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Delay before emitting, relative to the previous chunk.
    pub delay_ms: u64,
    /// Which stream the bytes go to.
    pub stream: Stream,
    /// The bytes (UTF-8).
    pub data: String,
}

/// Why a transcript could not be loaded or saved.
#[derive(Debug, thiserror::Error)]
pub enum FakeError {
    /// The fixture named a schema other than [`SCHEMA_VERSION`].
    #[error(
        "unsupported transcript schema {received:?}; expected {}",
        SCHEMA_VERSION
    )]
    Schema {
        /// The received `schema` value.
        received: String,
    },
    /// Malformed JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn schema_of(value: &serde_json::Value) -> String {
    value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>")
        .to_owned()
}

/// A recorded adapter session: the fixture the `fake` adapter replays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    /// Must equal [`SCHEMA_VERSION`].
    pub schema: String,
    /// Output chunks in order.
    pub chunks: Vec<Chunk>,
    /// Exit code the replay ends with.
    pub exit_code: i32,
}

impl Transcript {
    /// Loads a fixture, rejecting any schema but [`SCHEMA_VERSION`].
    ///
    /// # Errors
    /// Returns [`FakeError::Io`] when the file cannot be read,
    /// [`FakeError::Json`] on malformed JSON, and [`FakeError::Schema`] on
    /// a version mismatch.
    pub fn load(path: &Path) -> Result<Self, FakeError> {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let received = schema_of(&value);
        if received != SCHEMA_VERSION {
            return Err(FakeError::Schema { received });
        }
        Ok(serde_json::from_value(value)?)
    }

    /// Writes the fixture as pretty JSON.
    ///
    /// # Errors
    /// Propagates serialization and filesystem failures.
    pub fn save(&self, path: &Path) -> Result<(), FakeError> {
        std::fs::write(path, serde_json::to_vec_pretty(self)?).map_err(FakeError::Io)
    }

    /// Builds a transcript from a captured [`SpawnResult`].
    ///
    /// Chunk-level timing is not preserved; each captured stream becomes
    /// one chunk with no delay.
    #[must_use]
    pub fn from_result(result: &SpawnResult) -> Self {
        let chunk = |stream, bytes: &[u8]| Chunk {
            delay_ms: 0,
            stream,
            data: String::from_utf8_lossy(bytes).into_owned(),
        };
        let chunks = [
            chunk(Stream::Stdout, &result.stdout),
            chunk(Stream::Stderr, &result.stderr),
        ]
        .into_iter()
        .filter(|c| !c.data.is_empty())
        .collect();
        Self {
            schema: SCHEMA_VERSION.to_owned(),
            chunks,
            exit_code: result.exit_code.unwrap_or(1),
        }
    }
}

fn emit(chunk: &Chunk) {
    let bytes = chunk.data.as_bytes();
    match chunk.stream {
        Stream::Stdout => {
            let _ = std::io::stdout().lock().write_all(bytes);
            let _ = std::io::stdout().flush();
        }
        Stream::Stderr => {
            let _ = std::io::stderr().lock().write_all(bytes);
            let _ = std::io::stderr().flush();
        }
    }
}

/// Replays a transcript to this process's stdout and stderr, honoring
/// chunk delays, and returns the recorded exit code.
pub async fn replay(transcript: &Transcript) -> i32 {
    for chunk in &transcript.chunks {
        tokio::time::sleep(Duration::from_millis(chunk.delay_ms)).await;
        emit(chunk);
    }
    transcript.exit_code
}

/// Runs `request` under the layer-0 contract and returns the transcript a
/// `fake` adapter can replay later.
#[must_use]
pub async fn record(request: SpawnRequest) -> Transcript {
    Transcript::from_result(&spawn(request).await)
}

fn write_chunk_line(
    script: &mut String,
    transcript: &Transcript,
    i: usize,
    work_dir: &Path,
) -> std::io::Result<()> {
    use std::fmt::Write as _;
    let chunk = &transcript.chunks[i];
    if chunk.delay_ms > 0 {
        let _ = writeln!(
            script,
            "sleep {}.{:03}",
            chunk.delay_ms / 1000,
            chunk.delay_ms % 1000
        );
    }
    let path = work_dir.join(format!("chunk-{i}"));
    std::fs::write(&path, &chunk.data)?;
    let fd = match chunk.stream {
        Stream::Stdout => 1,
        Stream::Stderr => 2,
    };
    let _ = writeln!(script, "cat \"{}\" >&{fd}", path.display());
    Ok(())
}

/// Builds the layer-0 request that replays a transcript.
///
/// Uses only the shell and chunk files written under `work_dir` — the
/// fake adapter needs no bureau binary, so tests never depend on the
/// build layout. `secrets` is the run's scrub list, forwarded
/// unchanged: a fixture echoing a credential is redacted at the
/// capture boundary exactly like a real subprocess.
///
/// # Errors
/// Propagates filesystem failures writing chunk files.
pub fn replay_request(
    transcript: &Transcript,
    work_dir: &Path,
    request: &StepRequest,
    timeout: Duration,
    clock: fn() -> u64,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> std::io::Result<SpawnRequest> {
    use std::fmt::Write as _;
    let mut script = String::new();
    for i in 0..transcript.chunks.len() {
        write_chunk_line(&mut script, transcript, i, work_dir)?;
    }
    let _ = writeln!(script, "exit {}", transcript.exit_code);
    Ok(SpawnRequest {
        argv: vec!["sh".to_owned(), "-c".to_owned(), script],
        dir: request.worktree.clone(),
        env: std::collections::BTreeMap::new(),
        stdin: request.to_json().map_err(std::io::Error::other)?,
        timeout,
        secrets,
        clock,
        log,
    })
}

fn scratch_dir() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "bureau-fake-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn failed(message: &str) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: crate::contract::StepOutcome::Failure,
        outputs: std::collections::BTreeMap::new(),
        artifacts: Vec::new(),
        trust: crate::contract::Trust::Derived,
        cost_usd: 0.0,
        message: message.to_owned(),
    }
}

/// Runs a step through the fake adapter: replay the fixture named by
/// the step and derive the result.
///
/// The fixture path is a testing seam and must be absolute (config
/// validation enforces it). The run's scrub list reaches the replay
/// spawn unchanged, so fixture output cannot leak a credential into
/// the run log.
pub async fn execute(
    step: &StepDef,
    request: &StepRequest,
    clock: fn() -> u64,
    secrets: Vec<Secret>,
    log: Option<SharedLog>,
) -> StepResult {
    use crate::process::SpawnOutcome;
    let Some(fixture) = step.fixture.as_deref() else {
        return failed("fake adapter requires a `fixture` path on the agent step");
    };
    let transcript = match Transcript::load(Path::new(fixture)) {
        Ok(t) => t,
        Err(e) => return failed(&format!("loading fixture: {e}")),
    };
    let timeout = Duration::from_secs(step.timeout_secs.unwrap_or(300));
    let scratch = scratch_dir();
    let built = replay_request(&transcript, &scratch, request, timeout, clock, secrets, log);
    let result = match built {
        Ok(req) => spawn(req).await,
        Err(e) => return failed(&format!("preparing replay: {e}")),
    };
    let _ = tokio::fs::remove_dir_all(&scratch).await;
    if result.outcome == SpawnOutcome::Timeout {
        return failed("fake replay timed out");
    }
    super::result_from_spawn(&result)
}
