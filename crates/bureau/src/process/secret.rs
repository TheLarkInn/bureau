//! A credential value that cannot leak through formatting (DESIGN.md layer 0).

use std::fmt;

/// A secret value.
///
/// `Debug` prints `Secret(***)` and the buffer is wiped on drop, so a
/// secret is structurally unable to leak through `{:?}` or a panic message.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    /// Wraps a plaintext value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().into_bytes())
    }

    /// The plaintext. Only the scrubbing writer and environment injection
    /// may call this.
    #[must_use]
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("")
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Best effort: `unsafe_code` is forbidden in this workspace, so the
        // guaranteed-wipe `zeroize` crate is unavailable and the optimizer
        // may elide this fill. The redacting `Debug` is the hard guarantee.
        self.0.fill(0);
    }
}
