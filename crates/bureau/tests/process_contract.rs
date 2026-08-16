//! Layer 0 process contract tests: offline, `/bin/sh` only, no model
//! calls (DESIGN.md section 12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bureau::process::{
    self, REDACTED, Secret, SpawnOutcome, SpawnRequest, SpawnResult, shared_log, spawn,
};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An in-memory [`std::io::Write`] sink a test can read back.
#[derive(Clone, Default)]
struct MemLog(Arc<Mutex<Vec<u8>>>);

impl MemLog {
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().expect("mem log lock").clone()
    }
}

impl std::io::Write for MemLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("mem log lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn request(dir: &Path, script: &str, timeout: Duration) -> SpawnRequest {
    SpawnRequest {
        argv: vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
        dir: dir.to_path_buf(),
        env: BTreeMap::new(),
        stdin: Vec::new(),
        timeout,
        secrets: Vec::new(),
        log: None,
        cancel: None,
    }
}

async fn run(script: &str, timeout: Duration) -> SpawnResult {
    let dir = TestDir::new("run");
    spawn(request(dir.path(), script, timeout)).await
}

#[tokio::test]
async fn exit_code_and_captured_streams() {
    let result = run("echo out; echo err >&2; exit 3", Duration::from_secs(5)).await;
    let status = (result.outcome, result.exit_code, result.error.is_none());
    assert_eq!(status, (SpawnOutcome::Exited, Some(3), true));
    let out = String::from_utf8_lossy(&result.stdout);
    let err = String::from_utf8_lossy(&result.stderr);
    assert_eq!(format!("{out}|{err}"), "out\n|err\n");
}

#[tokio::test]
async fn stdin_is_delivered() {
    // 300KB exceeds both pipe buffers — guards the concurrent-writer fix.
    let dir = TestDir::new("stdin");
    let mut req = request(dir.path(), "cat", Duration::from_secs(5));
    req.stdin = vec![b'y'; 300_000];
    let result = spawn(req).await;
    let all_y = result.stdout.iter().all(|&b| b == b'y');
    let status = (result.outcome, result.stdout.len(), all_y);
    assert_eq!(status, (SpawnOutcome::Exited, 300_000, true));
}

#[tokio::test]
async fn timeout_covers_a_blocked_stdin_writer() {
    // `yes` floods stdout, never reads stdin: the 1MB write blocks forever;
    // only the timeout's group kill (which closes the pipes) can end it.
    let dir = TestDir::new("stdinblock");
    let mut req = request(dir.path(), "yes", Duration::from_millis(500));
    req.stdin = vec![b'x'; 1_000_000];
    let started = Instant::now();
    let result = spawn(req).await;
    let done = started.elapsed() < Duration::from_secs(10);
    assert_eq!((result.outcome, done), (SpawnOutcome::Timeout, true));
}

#[tokio::test]
async fn large_stdout_with_pending_stdin_is_captured() {
    // 200KB stdout with a full, unread stdin pipe: the writer must run
    // concurrently with the drains or the run stalls before the child exits.
    let dir = TestDir::new("bigout");
    let mut req = request(dir.path(), "yes | head -c 200000", Duration::from_secs(10));
    req.stdin = vec![b'x'; 100_000];
    let result = spawn(req).await;
    let status = (result.outcome, result.stdout.len());
    assert_eq!(status, (SpawnOutcome::Exited, 200_000));
}

#[tokio::test]
async fn env_is_exactly_what_was_passed() {
    let dir = TestDir::new("env");
    let mut req = request(dir.path(), "echo \"mark:$MARKER\"", Duration::from_secs(5));
    req.env.insert("MARKER".to_owned(), "present".to_owned());
    let result = spawn(req).await;
    assert_eq!(result.stdout, b"mark:present\n");
}

#[tokio::test]
async fn env_is_never_inherited() {
    // The runner has PATH/HOME set; an empty-env child must see neither.
    // (No in-process sentinel: `std::env::set_var` is `unsafe` on 2024.)
    let result = run("env", Duration::from_secs(5)).await;
    let env = String::from_utf8_lossy(&result.stdout);
    assert!(!env.contains("PATH="), "inherited PATH: {env}");
    assert!(!env.contains("HOME="), "inherited HOME: {env}");
}

#[tokio::test]
async fn missing_program_is_spawn_failed() {
    let dir = TestDir::new("spawnfail");
    let mut req = request(dir.path(), "", Duration::from_secs(5));
    req.argv = vec!["/definitely/not/a/binary".to_owned()];
    let result = spawn(req).await;
    let status = (result.outcome, result.exit_code, result.error.is_some());
    assert_eq!(status, (SpawnOutcome::SpawnFailed, None, true));
}

#[tokio::test]
async fn empty_argv_is_spawn_failed_before_spawn() {
    let dir = TestDir::new("emptyargv");
    let mut req = request(dir.path(), "", Duration::from_secs(5));
    req.argv = Vec::new();
    let result = spawn(req).await;
    assert_eq!(result.outcome, SpawnOutcome::SpawnFailed);
    assert_eq!(result.error.as_deref(), Some("argv is empty"));
}

#[tokio::test]
async fn signaled_is_distinct_from_exited() {
    let result = run("kill -TERM $$", Duration::from_secs(5)).await;
    let status = (result.outcome, result.exit_code);
    assert_eq!(status, (SpawnOutcome::Signaled, None));
}

#[tokio::test]
async fn timeout_hard_kills() {
    let started = Instant::now();
    let result = run("sleep 30", Duration::from_millis(500)).await;
    let status = (result.outcome, result.exit_code);
    assert_eq!(status, (SpawnOutcome::Timeout, None));
    assert!(started.elapsed() < Duration::from_secs(10));
}

fn pid_is_dead(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err()
}

fn wait_for_death(pid: i32) -> bool {
    for _ in 0..100 {
        if pid_is_dead(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[tokio::test]
async fn timeout_kills_the_whole_process_group() {
    let dir = TestDir::new("pgroup");
    let script = "sleep 30 & echo $! > grandchild.pid; wait";
    let result = spawn(request(dir.path(), script, Duration::from_millis(800))).await;
    let pid: i32 = std::fs::read_to_string(dir.path().join("grandchild.pid"))
        .expect("grandchild pidfile")
        .trim()
        .parse()
        .expect("pid is a number");
    let dead = wait_for_death(pid);
    if !dead {
        let pid = nix::unistd::Pid::from_raw(pid);
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
    }
    assert_eq!((result.outcome, dead), (SpawnOutcome::Timeout, true));
}

#[tokio::test]
async fn secrets_are_scrubbed_from_result_and_sink() {
    let dir = TestDir::new("scrub");
    let sink = MemLog::default();
    let script = "echo token is hunter2hunter2 ok; echo err hunter2hunter2 >&2";
    let mut req = request(dir.path(), script, Duration::from_secs(5));
    req.secrets = vec![Secret::new("hunter2hunter2")];
    req.log = Some(shared_log(sink.clone()));
    let result = spawn(req).await;
    let out = String::from_utf8_lossy(&result.stdout);
    let err = String::from_utf8_lossy(&result.stderr);
    let logged = sink.bytes();
    let combined = format!("{out}|{err}|{}", String::from_utf8_lossy(&logged));
    assert!(!combined.contains("hunter2hunter2"), "leaked: {combined}");
    assert_eq!(combined.matches(REDACTED).count(), 4, "{combined}");
}

#[tokio::test]
async fn secret_split_across_chunks_is_caught() {
    let result = split_secret_run().await;
    let out = String::from_utf8_lossy(&result.stdout);
    assert!(!out.contains("hunter2hunter2"), "secret leaked: {out}");
    assert!(out.contains(REDACTED));
}

async fn split_secret_run() -> SpawnResult {
    let dir = TestDir::new("split");
    let script = "printf 'hunter'; sleep 0.4; printf '2hunter2\\n'";
    let mut req = request(dir.path(), script, Duration::from_secs(5));
    req.secrets = vec![Secret::new("hunter2hunter2")];
    spawn(req).await
}

async fn first_output(sink: &MemLog) -> Option<Vec<u8>> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(800) {
        let logged = sink.bytes();
        if !logged.is_empty() {
            return Some(logged);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

#[tokio::test]
async fn output_streams_to_sink_before_exit() {
    let dir = TestDir::new("stream");
    let sink = MemLog::default();
    let script = "echo first; sleep 1; echo second";
    let mut req = request(dir.path(), script, Duration::from_secs(10));
    req.log = Some(shared_log(sink.clone()));
    let task = tokio::spawn(spawn(req));
    let logged = first_output(&sink)
        .await
        .expect("sink saw no output before the child exited");
    assert!(logged.starts_with(b"first\n"), "streamed: {logged:?}");
    assert!(task.await.is_ok());
}

#[test]
fn missing_credential_fails_with_its_name() {
    let error = process::resolve("no-such-credential").expect_err("must fail");
    let message = error.to_string();
    let named = message.contains("no-such-credential")
        && message.contains("BUREAU_CREDENTIAL_NO_SUCH_CREDENTIAL");
    assert!(named, "message: {message}");
}

#[test]
fn credential_resolves_from_a_file() {
    let dir = TestDir::new("creds");
    std::fs::write(dir.path().join("ado-main"), "  token-value\n").expect("write credential");
    let secret = process::resolve_file(dir.path(), "ado-main").expect("resolve");
    assert_eq!(secret.expose(), "token-value");
}

#[test]
fn secret_debug_is_redacted() {
    let debug = format!("{:?}", Secret::new("hunter2hunter2"));
    assert_eq!(debug, "Secret(***)");
}
