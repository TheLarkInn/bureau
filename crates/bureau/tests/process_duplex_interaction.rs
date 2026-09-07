//! The protocol callback shares process supervision from initialization to EOF.

mod process_duplex_support;

use std::task::Poll;
use std::time::Duration;

use bureau::process::{SpawnOutcome, duplex};

use process_duplex_support::{TestDir, ready};

#[tokio::test]
async fn completed_callback_is_dropped_before_waiting_for_eof_exit() {
    let dir = TestDir::new("callback-drop");
    let request = dir.request("touch ready; cat >/dev/null; exit 4");
    let exchange = duplex(request, |stdin, stdout| {
        std::future::poll_fn(move |_| {
            let _pipes = (&stdin, &stdout);
            Poll::Ready(Err::<(), &str>("protocol failure"))
        })
    });
    let (result, ()) = tokio::join!(exchange, ready(&dir));
    assert_eq!(
        (result.0.outcome, result.0.exit_code, result.1),
        (SpawnOutcome::Exited, Some(4), Some(Err("protocol failure")))
    );
}

#[tokio::test]
async fn timeout_supervises_a_callback_blocked_in_initialization() {
    let dir = TestDir::new("callback-timeout");
    let mut request = dir.request("touch ready; sleep 30");
    request.timeout = Duration::from_millis(200);
    let exchange = duplex(request, |stdin, stdout| async move {
        let _pipes = (stdin, stdout);
        std::future::pending::<()>().await;
    });
    let ((result, output), ()) = tokio::join!(exchange, ready(&dir));
    assert_eq!(
        (result.outcome, result.exit_code, output),
        (SpawnOutcome::Timeout, None, None)
    );
}

#[tokio::test]
async fn callback_completion_does_not_fabricate_an_exit_for_an_eof_ignoring_server() {
    let dir = TestDir::new("callback-shutdown");
    let (result, output) = duplex(dir.request("sleep 30"), |stdin, stdout| async move {
        drop((stdin, stdout));
        42
    })
    .await;
    assert_eq!(
        (
            result.outcome,
            result.exit_code,
            output,
            result.duration < Duration::from_secs(3)
        ),
        (SpawnOutcome::Signaled, None, Some(42), true)
    );
}
