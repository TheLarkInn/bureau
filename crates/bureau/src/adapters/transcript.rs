//! Recovering a step result from an agent CLI's captured output.
//!
//! A real agent CLI renders its tool transcript to stdout ahead of the
//! final message, so the contract document arrives surrounded by other
//! text. Layer 2 stays strict (`contract::StepResult::from_json`);
//! tolerating that surrounding text is this adapter family's policy,
//! not a property of schema v2, and deterministic steps keep the strict
//! parse so arbitrary command output cannot seize an outcome.

use crate::contract::{DecodeError, StepResult};

/// How much of the tail to search.
///
/// A step's answer is the CLI's last output, so the document is at the
/// end. Bounding the window keeps the expensive case — output holding
/// no document at all, which is exactly when every brace gets tried —
/// proportional to the window rather than to a transcript that may run
/// to megabytes.
const TAIL_LIMIT: usize = 256 * 1024;

/// Parses the first complete JSON value at the start of `bytes`,
/// tolerating trailing bytes after it.
fn leading_value(bytes: &[u8]) -> Option<serde_json::Value> {
    serde_json::Deserializer::from_slice(bytes)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

/// The last [`TAIL_LIMIT`] bytes of `bytes`.
fn searched_tail(bytes: &[u8]) -> &[u8] {
    let start = bytes.len().saturating_sub(TAIL_LIMIT);
    bytes.get(start..).unwrap_or(bytes)
}

/// Finds a result embedded in surrounding output.
///
/// Searches from the end, so the last valid document wins: an agent may
/// quote an example before answering, and the answer comes last. A
/// truncated final document would therefore let an earlier example win,
/// but only the caller below reaches here, and only after exit zero — a
/// CLI killed mid-write does not exit zero, so it takes the strict path.
///
/// Byte-wise throughout: `b'{'` is never a UTF-8 continuation byte, so a
/// window cut mid-character can neither panic nor invent a candidate.
fn embedded(bytes: &[u8]) -> Option<StepResult> {
    let searched = searched_tail(bytes);
    (0..searched.len())
        .rev()
        .filter(|index| searched.get(*index) == Some(&b'{'))
        .find_map(|start| StepResult::from_value(leading_value(searched.get(start..)?)?).ok())
}

/// Parses a result from captured process output that may surround the
/// document with other text.
///
/// Requiring the whole buffer to be one document discards results an
/// agent published correctly. A buffer that is exactly one document
/// still takes the strict path.
///
/// # Errors
/// Returns the strict [`StepResult::from_json`] error when no embedded
/// document parses either.
pub(super) fn result_from_output(bytes: &[u8]) -> Result<StepResult, DecodeError> {
    StepResult::from_json(bytes).or_else(|error| embedded(bytes).ok_or(error))
}
