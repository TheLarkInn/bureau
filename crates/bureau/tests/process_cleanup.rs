//! Child process groups do not survive normal parent exit or future cancellation.

use std::collections::BTreeMap;
use std::time::Duration;

use bureau::process::{SpawnOutcome, SpawnRequest, spawn};

#[tokio::test]
async fn normal_parent_exit_kills_background_descendants() {
    let dir = std::env::temp_dir().join(format!("bureau-cleanup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let marker = dir.join("late");
    let command = "(sleep 0.3; touch late) >/dev/null 2>&1 &";
    let result = spawn(SpawnRequest {
        argv: vec!["sh".to_owned(), "-c".to_owned(), command.to_owned()],
        dir: dir.clone(),
        env: BTreeMap::new(),
        stdin: Vec::new(),
        timeout: Duration::from_secs(5),
        secrets: Vec::new(),
        log: None,
        cancel: None,
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let seen = (result.outcome, result.exit_code, marker.exists());
    std::fs::remove_dir_all(dir).expect("cleanup");
    assert_eq!(seen, (SpawnOutcome::Exited, Some(0), false));
}

#[tokio::test]
async fn detached_pipe_holder_cannot_hang_completion() {
    let dir = std::env::temp_dir().join(format!("bureau-detached-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let started = std::time::Instant::now();
    let marker = dir.join("late");
    let result = spawn(detached_request(&dir)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let escaped = marker.exists();
    std::fs::remove_dir_all(dir).expect("cleanup");
    assert!(
        result.outcome == SpawnOutcome::Exited
            && started.elapsed() < Duration::from_secs(2)
            && !escaped
    );
}

fn detached_request(dir: &std::path::Path) -> SpawnRequest {
    SpawnRequest {
        argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "setsid sh -c 'sleep 0.3; touch late' &".to_owned(),
        ],
        dir: dir.to_path_buf(),
        env: BTreeMap::new(),
        stdin: Vec::new(),
        timeout: Duration::from_secs(5),
        secrets: Vec::new(),
        log: None,
        cancel: None,
    }
}
