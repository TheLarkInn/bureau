//! Durable cancellation marker kills an active process group.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use bureau::process::{SpawnOutcome, SpawnRequest, spawn};

#[tokio::test]
async fn cancellation_marker_stops_an_active_process() {
    let dir = temp_dir();
    let marker = dir.join("CANCEL");
    let request = SpawnRequest {
        argv: vec!["sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
        dir: dir.clone(),
        env: BTreeMap::new(),
        stdin: Vec::new(),
        timeout: Duration::from_secs(60),
        secrets: Vec::new(),
        log: None,
        cancel: Some(marker.clone()),
    };
    let task = tokio::spawn(spawn(request));
    tokio::time::sleep(Duration::from_millis(150)).await;
    std::fs::write(marker, "operator cancelled").expect("write marker");
    let result = task.await.expect("spawn task");
    let seen = (
        result.outcome,
        result.error.as_deref(),
        result.duration < Duration::from_secs(5),
    );
    assert_eq!(seen, (SpawnOutcome::Signaled, Some("cancelled"), true));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bureau-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
