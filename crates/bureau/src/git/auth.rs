//! Git-over-HTTPS credentials: the argv they ride in and every form
//! the scrub list must hold.

use crate::config::ForgeKind;
use crate::process::Secret;

/// A resolved credential for git-over-HTTPS auth.
pub struct Credential {
    user: &'static str,
    secret: Secret,
}

/// Maps a forge kind to its git-over-HTTPS credential shape.
#[must_use]
pub const fn credential_for(forge: ForgeKind, secret: Secret) -> Credential {
    let user = match forge {
        ForgeKind::Ado => "pat",
        ForgeKind::Github => "x-access-token",
    };
    Credential { user, secret }
}

/// Base64-encode without a dependency (the approved crate list has none).
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .fold(0usize, |acc, &b| (acc << 8) | usize::from(b))
            << (8 * (3 - chunk.len()));
        for i in 0..4 {
            let keep = chunk.len() + 1;
            out.push(if i < keep {
                char::from(TABLE[(n >> (18 - 6 * i)) & 63])
            } else {
                '='
            });
        }
    }
    out
}

/// The `-c http.extraheader=...` argv carrying `credential`.
///
/// Every form the credential takes joins the scrub list: the raw
/// secret, the base64 `user:secret` pair in argv and on the wire, and
/// the full `AUTHORIZATION: Basic` value an error page would echo.
#[must_use]
pub fn auth_args(credential: &Credential, secrets: &mut Vec<Secret>) -> Vec<String> {
    let user = credential.user;
    let pair = base64(format!("{user}:{}", credential.secret.expose()).as_bytes());
    secrets.push(credential.secret.clone());
    secrets.push(Secret::new(pair.as_str()));
    secrets.push(Secret::new(format!("AUTHORIZATION: Basic {pair}")));
    vec![
        "-c".to_owned(),
        format!("http.extraheader=AUTHORIZATION: Basic {pair}"),
    ]
}
