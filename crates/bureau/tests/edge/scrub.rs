//! `ScrubWriter` adversarial edges (DESIGN.md layer 0): the drain
//! boundary, secrets larger than a chunk, nested secrets, binary input.

use std::io::Write as _;

use bureau::process::{REDACTED, ScrubWriter, Secret};

fn secrets(values: &[&str]) -> Vec<Secret> {
    values.iter().map(|v| Secret::new(*v)).collect()
}

fn scrubbed(chunks: &[&[u8]], secrets: &[Secret]) -> Vec<u8> {
    let mut writer = ScrubWriter::new(Vec::new(), secrets);
    for chunk in chunks {
        writer.write_all(chunk).expect("write to Vec");
    }
    writer.finish().expect("finish")
}

#[test]
fn secret_ending_at_the_8192_drain_boundary_is_held() {
    let secret = "sixteen-byte key";
    let pad = 8192 - secret.len();
    let mut first = vec![b'x'; pad];
    first.extend_from_slice(secret.as_bytes());
    let mut writer = ScrubWriter::new(Vec::new(), &secrets(&[secret]));
    writer.write_all(&first).expect("write");
    // The secret ends exactly where a drain chunk cuts; the crossing
    // check must hold back all of it, emitting only the padding.
    assert_eq!(writer.get_ref().len(), pad);
    let out = writer.finish().expect("finish");
    assert_eq!(out, format!("{}{REDACTED}", "x".repeat(pad)).as_bytes());
}

#[test]
fn secret_longer_than_one_chunk_is_caught() {
    let secret = "ab".repeat(4600); // 9200 bytes > the 8192 drain chunk
    let body = format!("prefix{secret}suffix");
    let (a, b) = body.split_at(8192);
    let out = scrubbed(&[a.as_bytes(), b.as_bytes()], &secrets(&[&secret]));
    assert_eq!(out, format!("prefix{REDACTED}suffix").as_bytes());
}

#[test]
fn two_adjacent_secrets_split_across_writes_are_each_caught() {
    // The seam falls between the secrets; the crossing pullback must not
    // emit a partial second secret.
    let out = scrubbed(&[b"xaa", b"abbbz"], &secrets(&["aaa", "bbb"]));
    assert_eq!(out, format!("x{REDACTED}{REDACTED}z").as_bytes());
}

#[test]
fn a_suffix_secret_never_shadows_the_longer_one() {
    // Scanning is by start position, so the suffix "defgh" can only match
    // at its own start; either list order redacts both fully.
    let orders: [&[&str]; 2] = [&["abcdefgh", "defgh"], &["defgh", "abcdefgh"]];
    for order in orders {
        let out = scrubbed(&[b"xabcdefghy defgh!"], &secrets(order));
        let expected = format!("x{REDACTED}y {REDACTED}!");
        assert_eq!(out, expected.as_bytes(), "{order:?}");
    }
}

#[test]
fn invalid_utf8_passes_through_byte_for_byte() {
    let mut input = vec![0xFF, 0x80, 0x00];
    input.extend_from_slice(b"topsecret");
    input.extend_from_slice(&[0xFE, 0xC3, 0x28]);
    let out = scrubbed(&[&input], &secrets(&["topsecret"]));
    let mut expected = vec![0xFF, 0x80, 0x00];
    expected.extend_from_slice(REDACTED.as_bytes());
    expected.extend_from_slice(&[0xFE, 0xC3, 0x28]);
    assert_eq!(out, expected);
    assert!(std::str::from_utf8(&out).is_err(), "still not UTF-8");
}

#[test]
fn redacted_text_in_input_is_not_rescrubbed() {
    // The marker is only a replacement string; verbatim occurrences in
    // the child's own output pass through, never eaten or doubled.
    let out = scrubbed(&[b"token [REDACTED] already"], &secrets(&["hunter2"]));
    assert_eq!(out, b"token [REDACTED] already");
}
