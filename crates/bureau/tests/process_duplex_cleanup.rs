//! EOF, deadline, cancellation, and dropped futures never leave protocol descendants.

mod process_duplex_support;

use std::time::{Duration, Instant};

use bureau::process::{Duplex, SpawnOutcome, SpawnRequest, SpawnResult, start_duplex};

use process_duplex_support::{TestDir, ready};

const TREE: &str =
    "setsid sh -c 'touch ready; sleep 0.8; touch escaped' </dev/null >/dev/null 2>&1 &";

async fn late_marker(dir: &TestDir) -> bool {
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    dir.0.join("escaped").exists()
}

async fn completed(
    waiting: impl std::future::Future<Output = SpawnResult>,
    dir: &TestDir,
) -> (SpawnResult, bool) {
    let result = waiting.await;
    let escaped = late_marker(dir).await;
    (result, escaped)
}

#[tokio::test]
async fn normal_eof_exit_kills_detached_descendants() {
    let dir = TestDir::new("normal-descendants");
    let request = dir.request(&format!("{TREE} read answer"));
    let Duplex {
        stdin,
        stdout,
        owner,
    } = start_duplex(request).expect("spawn");
    ready(&dir).await;
    drop((stdin, stdout));
    let (result, escaped) = completed(owner.finish(), &dir).await;
    assert_eq!(
        (result.outcome, result.exit_code, escaped),
        (SpawnOutcome::Exited, Some(1), false)
    );
}

#[tokio::test]
async fn eof_ignoring_server_has_bounded_teardown_and_no_fake_exit() {
    let dir = TestDir::new("ignores-eof");
    let Duplex {
        stdin,
        stdout,
        owner,
    } = start_duplex(dir.request("touch ready; sleep 30")).expect("spawn");
    ready(&dir).await;
    drop((stdin, stdout));
    let started = Instant::now();
    let result = owner.finish().await;
    assert_eq!(
        (
            result.outcome,
            result.exit_code,
            result.error.is_some(),
            started.elapsed() < Duration::from_secs(3)
        ),
        (SpawnOutcome::Signaled, None, true, true)
    );
}

async fn interrupted(request: SpawnRequest, dir: &TestDir) -> SpawnResult {
    let duplex = start_duplex(request).expect("spawn");
    ready(dir).await;
    resume_after_interrupted_wait(duplex).await
}

async fn resume_after_interrupted_wait(mut duplex: Duplex) -> SpawnResult {
    if let Ok(result) = tokio::time::timeout(Duration::from_millis(20), duplex.owner.wait()).await {
        return result;
    }
    drop((duplex.stdin, duplex.stdout));
    duplex.owner.finish().await
}

#[tokio::test]
async fn finishing_a_cancelled_wait_keeps_the_original_deadline() {
    let dir = TestDir::new("cancel-wait");
    let mut request = dir.request(&format!("{TREE} wait"));
    request.timeout = Duration::from_millis(200);
    let result = interrupted(request, &dir).await;
    let escaped = late_marker(&dir).await;
    assert_eq!(
        (result.outcome, result.exit_code, escaped),
        (SpawnOutcome::Timeout, None, false)
    );
}

async fn abort_interaction(dir: &TestDir) {
    let request = dir.request(&format!("{TREE} wait"));
    let interaction = tokio::spawn(bureau::process::duplex(
        request,
        |stdin, stdout| async move {
            let _pipes = (stdin, stdout);
            std::future::pending::<()>().await;
        },
    ));
    ready(dir).await;
    interaction.abort();
    let error = interaction.await.expect_err("interaction was aborted");
    assert!(error.is_cancelled());
}

#[tokio::test]
async fn dropped_interaction_future_kills_detached_descendants() {
    let dir = TestDir::new("aborted-interaction");
    abort_interaction(&dir).await;
    assert!(!late_marker(&dir).await);
}

#[tokio::test]
async fn dropped_owner_before_wait_is_polled_kills_detached_descendants() {
    let dir = TestDir::new("unpolled-owner");
    let duplex = start_duplex(dir.request(&format!("{TREE} wait"))).expect("spawn");
    ready(&dir).await;
    drop(duplex.owner);
    let escaped = late_marker(&dir).await;
    drop((duplex.stdin, duplex.stdout));
    assert!(!escaped);
}

#[tokio::test]
async fn cancellation_kills_detached_descendants() {
    let dir = TestDir::new("cancel-descendants");
    let mut request = dir.request(&format!("{TREE} wait"));
    request.cancel = Some(dir.0.join("CANCEL"));
    let mut duplex = start_duplex(request).expect("spawn");
    ready(&dir).await;
    std::fs::write(dir.0.join("CANCEL"), "").expect("cancel");
    let (result, escaped) = completed(duplex.owner.wait(), &dir).await;
    assert_eq!(
        (result.error.as_deref(), escaped),
        (Some("cancelled"), false)
    );
}
