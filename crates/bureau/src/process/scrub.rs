//! The write boundary at which secrets are scrubbed (DESIGN.md layer 0).

use std::io::{self, Write};

use super::secret::Secret;

/// Replacement text for a scrubbed secret value.
pub const REDACTED: &str = "[REDACTED]";

/// A writer that removes secret values before bytes reach the inner sink.
///
/// Scrubbing happens on write, never on read. A tail of
/// `max_secret_len - 1` bytes is retained across writes so a secret split
/// across two chunks is still caught.
pub struct ScrubWriter<W> {
    inner: W,
    secrets: Vec<Vec<u8>>,
    holdback: usize,
    pending: Vec<u8>,
}

impl<W: Write> ScrubWriter<W> {
    /// Creates a scrubber for `secrets`. Empty secrets are ignored.
    #[must_use]
    pub fn new(inner: W, secrets: &[Secret]) -> Self {
        let secrets: Vec<Vec<u8>> = secrets
            .iter()
            .map(|s| s.expose().as_bytes().to_vec())
            .filter(|s| !s.is_empty())
            .collect();
        let holdback = secrets
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .saturating_sub(1);
        Self {
            inner,
            secrets,
            holdback,
            pending: Vec::new(),
        }
    }

    /// The wrapped writer.
    #[must_use]
    pub const fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Scrubs and writes any retained tail, then returns the inner writer.
    ///
    /// # Errors
    /// Propagates `inner` write and flush failures.
    pub fn finish(mut self) -> io::Result<W> {
        let out = self.drain_emittable(true);
        self.inner.write_all(&out)?;
        self.inner.flush()?;
        Ok(self.inner)
    }

    fn drain_emittable(&mut self, last: bool) -> Vec<u8> {
        let limit = self.emit_limit(last);
        let (mut out, mut i) = (Vec::new(), 0);
        while i < limit {
            if let Some(len) = self.secret_at(i, limit) {
                out.extend_from_slice(REDACTED.as_bytes());
                i += len;
            } else {
                out.push(self.pending[i]);
                i += 1;
            }
        }
        self.pending.drain(..i);
        out
    }

    /// How much of `pending` may be emitted: everything but the holdback
    /// tail, pulled back past the start of any secret that crosses it.
    fn emit_limit(&self, last: bool) -> usize {
        let len = self.pending.len();
        let mut limit = if last {
            len
        } else {
            len.saturating_sub(self.holdback)
        };
        while let Some(start) = self.crossing_start(limit) {
            limit = start;
        }
        limit
    }

    /// The smallest start of a secret occurrence straddling `limit`.
    fn crossing_start(&self, limit: usize) -> Option<usize> {
        self.secrets
            .iter()
            .flat_map(|s| {
                occurrences(&self.pending, s)
                    .into_iter()
                    .map(move |i| (i, s.len()))
            })
            .filter(|&(start, len)| start < limit && start + len > limit)
            .map(|(start, _)| start)
            .min()
    }

    /// Length of a secret starting at `from` and ending within `limit`.
    fn secret_at(&self, from: usize, limit: usize) -> Option<usize> {
        self.secrets
            .iter()
            .find(|s| from + s.len() <= limit && self.pending[from..].starts_with(s))
            .map(Vec::len)
    }
}

impl<W: Write> Write for ScrubWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        let out = self.drain_emittable(false);
        self.inner.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Start positions of every occurrence of `needle` in `haystack`.
fn occurrences(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle)
        .map(|(i, _)| i)
        .collect()
}
