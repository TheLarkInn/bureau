//! The `fake` adapter: replays a recorded transcript from a fixture file
//! (DESIGN.md layer 1). `record` captures a real subprocess through the
//! layer-0 contract and writes such a fixture.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::contract::SCHEMA_VERSION;
use crate::process::{SpawnRequest, SpawnResult, spawn};

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

/// Which stream a chunk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
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

fn schema_of(value: &serde_json::Value) -> String {
    value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>")
        .to_owned()
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

/// Runs `request` under the layer-0 contract and returns the transcript a
/// `fake` adapter can replay later.
#[must_use]
pub async fn record(request: SpawnRequest) -> Transcript {
    Transcript::from_result(&spawn(request).await)
}
