//! ADO Basic authentication: empty user, PAT as password, never logged.

use crate::process::Secret;

/// RFC 4648 base64; a local twin of `git.rs`'s private encoder.
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    data.chunks(3)
        .flat_map(|chunk| {
            let bits = chunk
                .iter()
                .fold(0usize, |acc, &byte| (acc << 8) | usize::from(byte))
                << (8 * (3 - chunk.len()));
            (0..4).map(move |i| {
                if i <= chunk.len() {
                    char::from(TABLE[(bits >> (18 - 6 * i)) & 63])
                } else {
                    '='
                }
            })
        })
        .collect()
}

/// The `Authorization` header value for one resolved credential.
pub(super) fn basic_auth(token: &Secret) -> String {
    format!(
        "Basic {}",
        base64(format!(":{}", token.expose()).as_bytes())
    )
}
