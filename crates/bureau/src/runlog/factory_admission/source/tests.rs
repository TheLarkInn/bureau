use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::{FactoryHistory, decode};
use super::FactorySource;
use crate::runlog::{Event, EventKind, output, run_started};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "bureau-source-memory-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).expect("source root");
    root
}

fn event(seq: u64, kind: EventKind, data: serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&Event {
        seq,
        at_ms: 0,
        kind,
        data,
    })
    .expect("event");
    bytes.push(b'\n');
    bytes
}

fn history(root: &Path, index: usize) {
    let name = format!("history-{index}");
    let directory = root.join(&name);
    std::fs::create_dir(&directory).expect("run directory");
    let mut file = std::fs::File::create(directory.join("events.jsonl")).expect("log");
    file.write_all(&event(
        0,
        EventKind::RunStarted,
        run_started(&name, "ordinary"),
    ))
    .expect("header");
    let data = output(None, "combined", &"x".repeat(512));
    for seq in 1..=256 {
        file.write_all(&event(seq, EventKind::Output, data.clone()))
            .expect("output");
    }
}

#[test]
fn source_capture_does_not_retain_all_historical_output_bytes() {
    let root = root();
    for index in 0..16 {
        history(&root, index);
    }
    let source = FactorySource::read(&root).expect("capture sources");
    let retained = source.buffered_bytes();
    std::fs::remove_dir_all(root).expect("remove fixture");
    assert_eq!(retained, 0, "historical output bytes must be streamed");
}

fn append(root: &Path, index: usize, seq: u64) {
    let line = event(
        seq,
        EventKind::Output,
        output(None, "combined", &"x".repeat(128 * 1024)),
    );
    let path = root.join(format!("history-{index}/events.jsonl"));
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("log")
        .write_all(&line)
        .expect("append output");
}

fn append_all(root: &Path, seq: u64) {
    for index in 0..16 {
        append(root, index, seq);
    }
}

#[test]
fn prepared_and_current_sources_share_one_global_tail_buffer_bound() {
    let root = root();
    for index in 0..16 {
        history(&root, index);
    }
    let source = FactoryHistory::prepare(FactorySource::read(&root).expect("source"), None);
    append_all(&root, 257);
    let current = source.capture(&root).expect("first catch-up");
    let source = FactoryHistory::prepare(current, Some(source));
    append_all(&root, 258);
    let current = source.capture(&root).expect("fenced recapture");
    let retained = source.source.buffered_bytes() + current.buffered_bytes();
    std::fs::remove_dir_all(root).expect("remove fixture");
    assert!(retained <= 1024 * 1024, "retained {retained} tail bytes");
}

#[test]
fn replayed_bytes_must_match_the_fenced_digest_even_after_an_aba_restore() {
    let root = root();
    history(&root, 0);
    let path = root.join("history-0/events.jsonl");
    let bytes = std::fs::read(&path).expect("original bytes");
    let source = FactorySource::read(&root).expect("captured source");
    let mut changed = bytes.clone();
    changed[0] = b'!';
    std::fs::write(&path, changed).expect("change outside capture");
    let state = decode::prepare(&source.runs[0], None);
    std::fs::write(&path, bytes).expect("restore captured bytes");
    let prepared = FactoryHistory {
        source,
        states: vec![state],
    };
    let current = prepared.capture(&root).expect("current original bytes");
    let accepted = prepared.matches(&current);
    std::fs::remove_dir_all(root).expect("remove fixture");
    assert!(
        !accepted,
        "replay of different bytes must not borrow the restored digest"
    );
}

#[test]
fn malformed_replay_is_bound_only_after_hashing_the_complete_range() {
    let root = root();
    history(&root, 0);
    let path = root.join("history-0/events.jsonl");
    let mut bytes = std::fs::read(&path).expect("original bytes");
    bytes[0] = b'!';
    std::fs::write(&path, bytes).expect("stable malformed source");
    let source = FactorySource::read(&root).expect("captured source");
    let prepared = decode::prepare(&source.runs[0], None);
    let actual = (prepared.bound(), prepared.state().is_err());
    std::fs::remove_dir_all(root).expect("remove fixture");
    assert_eq!(actual, (true, true));
}

#[test]
fn full_replay_stops_at_the_captured_length_when_output_arrives_before_decoding() {
    let root = root();
    history(&root, 0);
    let source = FactorySource::read(&root).expect("captured source");
    append(&root, 0, 257);
    let prepared = FactoryHistory::prepare(source, None);
    let current = prepared.capture(&root).expect("current prefix and output");
    let accepted = prepared.matches(&current);
    std::fs::remove_dir_all(root).expect("remove fixture");
    assert!(
        accepted,
        "later output must not invalidate the captured replay range"
    );
}

#[test]
fn incremental_replay_rejects_an_aba_suffix_before_binding_its_state() {
    let root = root();
    history(&root, 0);
    let previous = FactoryHistory::prepare(FactorySource::read(&root).expect("source"), None);
    append(&root, 0, 257);
    let source = previous.capture(&root).expect("captured append");
    let path = root.join("history-0/events.jsonl");
    let bytes = std::fs::read(&path).expect("original bytes");
    let mut changed = bytes.clone();
    let position = changed
        .iter()
        .rposition(|byte| *byte == b'x')
        .expect("output text");
    changed[position] = b'y';
    std::fs::write(&path, changed).expect("changed append");
    let prepared = FactoryHistory::prepare(source, Some(previous));
    std::fs::write(&path, bytes).expect("restore captured append");
    let current = prepared.capture(&root).expect("current original bytes");
    let accepted = prepared.matches(&current);
    std::fs::remove_dir_all(root).expect("remove fixture");
    assert!(
        !accepted,
        "a changed suffix must not borrow the restored digest"
    );
}
