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
    let captured = format!(
        "{}|{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(captured, "out\n|err\n");
}

#[tokio::test]
async fn stdin_is_delivered() {
    let dir = TestDir::new("stdin");
    let mut req = request(dir.path(), "cat", Duration::from_secs(5));
    req.stdin = b"hello stdin".to_vec();
    let result = spawn(req).await;
    assert_eq!(result.outcome, SpawnOutcome::Exited);
    assert_eq!(result.stdout, b"hello stdin");
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
    // The test runner process certainly has PATH and HOME set; a child
    // spawned with an empty env must see neither. (A sentinel var cannot
    // be set in-process: `std::env::set_var` is `unsafe` on edition 2024
    // and this workspace forbids unsafe code.)
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
    assert_eq!(result.outcome, SpawnOutcome::SpawnFailed);
    assert_eq!(result.exit_code, None);
    assert!(result.error.is_some());
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
    assert_eq!(result.outcome, SpawnOutcome::Signaled);
    assert_eq!(result.exit_code, None);
}

#[tokio::test]
async fn timeout_hard_kills() {
    let started = Instant::now();
    let result = run("sleep 30", Duration::from_millis(500)).await;
    assert_eq!(result.outcome, SpawnOutcome::Timeout);
    assert_eq!(result.exit_code, None);
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
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    assert_eq!(
        (result.outcome, dead),
        (SpawnOutcome::Timeout, true),
        "grandchild survived"
    );
}

#[tokio::test]
async fn secrets_are_scrubbed_from_result_and_sink() {
    let dir = TestDir::new("scrub");
    let sink = MemLog::default();
    let mut req = request(
        dir.path(),
        "echo token is hunter2hunter2 ok; echo err hunter2hunter2 >&2",
        Duration::from_secs(5),
    );
    req.secrets = vec![Secret::new("hunter2hunter2")];
    req.log = Some(shared_log(sink.clone()));
    let result = spawn(req).await;
    let combined = format!(
        "{}|{}|{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&sink.bytes())
    );
    assert!(
        !combined.contains("hunter2hunter2"),
        "secret leaked: {combined}"
    );
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
    let mut req = request(
        dir.path(),
        "printf 'hunter'; sleep 0.4; printf '2hunter2\\n'",
        Duration::from_secs(5),
    );
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
    let mut req = request(
        dir.path(),
        "echo first; sleep 1; echo second",
        Duration::from_secs(10),
    );
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
    assert!(message.contains("no-such-credential"), "message: {message}");
    assert!(
        message.contains("BUREAU_CREDENTIAL_NO_SUCH_CREDENTIAL"),
        "message: {message}"
    );
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
    assert_eq!(
        format!("{:?}", Secret::new("hunter2hunter2")),
        "Secret(***)"
    );
}
