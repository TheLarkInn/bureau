//! Process-spawn adversarial edges (DESIGN.md layer 0): oversized
//! stdin and externally killed children.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use bureau::process::{SpawnOutcome, SpawnRequest, SpawnResult, spawn};

use super::testdir::TestDir;

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
async fn stdin_larger_than_the_pipe_buffer_is_delivered() {
    let dir = TestDir::new("bigstdin");
    let mut req = request(dir.path(), "cat > /dev/null", Duration::from_secs(10));
    req.stdin = vec![b'x'; 1024 * 1024];
    let result = spawn(req).await;
    // 1MB is ~16x the 64KB pipe buffer; the child drains stdin
    // continuously, so the parent's write completes.
    let status = (result.outcome, result.exit_code, result.stdout.is_empty());
    assert_eq!(status, (SpawnOutcome::Exited, Some(0), true));
}

#[tokio::test]
async fn a_sigkilled_child_is_signaled_with_no_exit_code() {
    let result = run("kill -9 $$", Duration::from_secs(5)).await;
    let status = (result.outcome, result.exit_code, result.error.as_deref());
    assert_eq!(status, (SpawnOutcome::Signaled, None, Some("signal 9")));
}

// Regression: the stdin writer must run concurrently with the stream
// drains under the timeout, so a child that fills its stdout pipe before
// reading stdin cannot deadlock the run.
#[tokio::test]
async fn a_full_stdout_pipe_cannot_deadlock_the_stdin_write() {
    let dir = TestDir::new("deadlock");
    let script = "head -c 200000 < /dev/zero; cat > /dev/null";
    let mut req = request(dir.path(), script, Duration::from_secs(5));
    req.stdin = vec![b'y'; 256 * 1024];
    let result = spawn(req).await;
    assert_eq!(result.outcome, SpawnOutcome::Exited);
}
