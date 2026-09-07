//! Offline duplex transport, deadline, and scrubbed stderr contracts.

mod process_duplex_support;

use std::time::{Duration, Instant};

use bureau::process::{
    Duplex, Secret, SpawnOutcome, SpawnRequest, SpawnResult, shared_log, start_duplex,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use process_duplex_support::{MemLog, TestDir, ready};

#[tokio::test]
async fn protocol_pipes_preserve_bytes_and_actual_exit_status() {
    let dir = TestDir::new("exchange");
    let script = "IFS= read -r line; printf 'reply:%s\\n' \"$line\"; cat >/dev/null; exit 7";
    let mut process = start_duplex(dir.request(script)).expect("spawn");
    let response = echo(&mut process).await;
    drop((process.stdin, process.stdout));
    let result = process.owner.finish().await;
    let actual = (
        response.as_str(),
        result.outcome,
        result.exit_code,
        result.error,
    );
    assert_eq!(
        actual,
        ("reply:hello\n", SpawnOutcome::Exited, Some(7), None)
    );
}

async fn echo(process: &mut Duplex) -> String {
    process.stdin.write_all(b"hello\n").await.expect("write");
    let mut output = BufReader::new(&mut process.stdout);
    let mut response = String::new();
    output
        .read_line(&mut response)
        .await
        .expect("read response");
    response
}

#[tokio::test]
async fn explicit_environment_never_inherits_parent_credentials() {
    let dir = TestDir::new("environment");
    let mut request = dir.request("/usr/bin/env");
    request
        .env
        .insert("EXPLICIT".to_owned(), "present".to_owned());
    let (result, output) = capture(start_duplex(request).expect("spawn")).await;
    let observed = (
        output.contains("EXPLICIT=present"),
        output.contains("PATH=") || output.contains("HOME="),
        result.exit_code,
    );
    assert_eq!(observed, (true, false, Some(0)));
}

#[tokio::test]
async fn stderr_is_scrubbed_and_streamed_while_protocol_remains_raw() {
    let dir = TestDir::new("stderr");
    let sink = MemLog::default();
    let mut process = start_duplex(stderr_request(&dir, &sink)).expect("spawn");
    let streamed = resume(&mut process, &dir, &sink).await;
    let (result, raw) = capture(process).await;
    let actual = (
        streamed.starts_with(b"[REDACTED]"),
        result.stderr,
        raw.as_str(),
    );
    assert_eq!(
        actual,
        (true, b"[REDACTED] diagnostics\n".to_vec(), "hunter2hunter2")
    );
}

async fn resume(process: &mut Duplex, dir: &TestDir, sink: &MemLog) -> Vec<u8> {
    let streamed = streamed(dir, sink).await;
    process.stdin.write_all(b"continue\n").await.expect("write");
    streamed
}

fn stderr_request(dir: &TestDir, sink: &MemLog) -> SpawnRequest {
    let mut request = dir.request(
        "printf hunter >&2; sleep 0.1; printf '2hunter2 diagnostics\\n' >&2; \
         touch ready; read answer; printf hunter2hunter2",
    );
    request.secrets = vec![Secret::new("hunter2hunter2")];
    request.log = Some(shared_log(sink.clone()));
    request
}

async fn streamed(dir: &TestDir, sink: &MemLog) -> Vec<u8> {
    ready(dir).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    sink.0.lock().expect("log lock").clone()
}

async fn capture(mut process: Duplex) -> (SpawnResult, String) {
    drop(process.stdin);
    let mut raw = String::new();
    let (result, read) = tokio::join!(
        process.owner.wait(),
        process.stdout.read_to_string(&mut raw)
    );
    read.expect("protocol output");
    (result, raw)
}

#[tokio::test]
async fn timeout_includes_initialization_before_wait_is_polled() {
    let dir = TestDir::new("deadline");
    let mut request = dir.request("touch ready; sleep 30");
    request.timeout = Duration::from_millis(250);
    let mut duplex = start_duplex(request).expect("spawn");
    past_deadline(&dir).await;
    let waiting = Instant::now();
    let result = duplex.owner.wait().await;
    assert_eq!(
        (
            result.outcome,
            result.exit_code,
            waiting.elapsed() < Duration::from_millis(200)
        ),
        (SpawnOutcome::Timeout, None, true)
    );
}

async fn past_deadline(dir: &TestDir) {
    ready(dir).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test]
async fn cancellation_interrupts_initialization() {
    let dir = TestDir::new("cancellation");
    let marker = dir.0.join("CANCEL");
    let mut request = dir.request("touch ready; sleep 30");
    request.cancel = Some(marker.clone());
    let mut duplex = start_duplex(request).expect("spawn");
    ready(&dir).await;
    std::fs::write(marker, "cancelled").expect("cancel marker");
    let result = duplex.owner.wait().await;
    assert_eq!(
        (result.outcome, result.exit_code, result.error.as_deref()),
        (SpawnOutcome::Signaled, None, Some("cancelled"))
    );
}

#[tokio::test]
async fn unexpected_exit_is_not_a_successful_protocol_response() {
    let dir = TestDir::new("unexpected-exit");
    for (script, expected) in [
        ("exit 9", (SpawnOutcome::Exited, Some(9))),
        ("kill -TERM $$", (SpawnOutcome::Signaled, None)),
    ] {
        let mut duplex = start_duplex(dir.request(script)).expect("spawn");
        let result = duplex.owner.wait().await;
        assert_eq!((result.outcome, result.exit_code), expected);
    }
}

#[test]
fn invalid_requests_fail_before_spawning() {
    let dir = TestDir::new("invalid");
    let mut requests = [dir.request("touch spawned"), dir.request("touch spawned")];
    requests[0].stdin = b"unexpected preloaded stdin".to_vec();
    requests[1].argv = vec!["/nonexistent/bureau-duplex-program".to_owned()];
    for request in requests {
        let result = start_duplex(request).err().expect("invalid request");
        assert_eq!(
            (
                result.outcome,
                result.exit_code,
                result.error.is_some(),
                dir.0.join("spawned").exists()
            ),
            (SpawnOutcome::SpawnFailed, None, true, false)
        );
    }
}
