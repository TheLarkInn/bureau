//! Scrubbing-writer tests (DESIGN.md layer 0): scrub on write, and the
//! retained tail that catches a secret split across two writes.

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
fn passes_through_when_no_secrets() {
    let out = scrubbed(&[b"hello ", b"world"], &[]);
    assert_eq!(out, b"hello world");
}

#[test]
fn removes_a_secret() {
    let out = scrubbed(&[b"token is abcdefgh ok"], &secrets(&["abcdefgh"]));
    assert_eq!(out, format!("token is {REDACTED} ok").as_bytes());
}

#[test]
fn catches_a_secret_split_across_two_writes() {
    let secret = "hunter2hunter2";
    let (first, second) = secret.split_at(6);
    let out = scrubbed(&[first.as_bytes(), second.as_bytes()], &secrets(&[secret]));
    assert_eq!(out, REDACTED.as_bytes());
}

#[test]
fn catches_every_split_point() {
    let secret = "s3cr3t-value";
    for cut in 1..secret.len() {
        let (first, second) = secret.split_at(cut);
        let out = scrubbed(&[first.as_bytes(), second.as_bytes()], &secrets(&[secret]));
        assert_eq!(out, REDACTED.as_bytes(), "split at {cut}");
    }
}

#[test]
fn catches_repeated_and_overlapping_occurrences() {
    let out = scrubbed(&[b"ab ab ab"], &secrets(&["ab"]));
    assert_eq!(out, format!("{REDACTED} {REDACTED} {REDACTED}").as_bytes());
}

#[test]
fn holds_back_a_tail_and_still_scrubs_on_completion() {
    let mut writer = ScrubWriter::new(Vec::new(), &secrets(&["hunter2"]));
    writer.write_all(b"data hunt").expect("write");
    // The tail of `len - 1` bytes is retained: "hunt" might yet grow into
    // the secret, so only "dat" may leave.
    assert_eq!(writer.get_ref(), &b"dat");
    writer.write_all(b"er2!").expect("write");
    assert_eq!(writer.get_ref(), &b"data ");
    assert_eq!(writer.finish().expect("finish"), b"data [REDACTED]!");
}

#[test]
fn ignores_empty_secrets() {
    let out = scrubbed(&[b"untouched"], &secrets(&[""]));
    assert_eq!(out, b"untouched");
}

#[test]
fn write_reports_the_full_input_len() {
    let mut writer = ScrubWriter::new(Vec::new(), &secrets(&["abcdefgh"]));
    assert_eq!(writer.write(b"xxabcdefgh").expect("write"), 10);
}
