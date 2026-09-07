use super::test_peer::{self, Case};

#[tokio::test]
async fn official_peer_streams_scrubbed_text_and_cumulative_cost() {
    let (result, text, usage) = test_peer::run(Case::Complete).await;
    result.expect("ACP exchange");
    assert_eq!(
        (text, usage.cost_usd, usage.input_tokens),
        (
            b"prefix [REDACTED] \xe2\x82\xac suffix".to_vec(),
            Some(0.4),
            None
        )
    );
}

#[tokio::test]
async fn protocol_and_selection_failures_do_not_become_success() {
    for case in [
        Case::MissingAgent,
        Case::RefusedAgent,
        Case::Drift,
        Case::Error,
        Case::Cancelled,
    ] {
        assert!(test_peer::run(case).await.0.is_err(), "{case:?}");
    }
}

#[tokio::test]
async fn extra_permissions_are_rejected_including_allow_only_requests() {
    for case in [Case::Permission, Case::NoRejection] {
        assert!(test_peer::run(case).await.0.is_ok());
    }
}

#[tokio::test]
async fn eof_during_prompt_is_not_completion() {
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), test_peer::run(Case::Eof))
        .await
        .expect("EOF must not hang");
    assert!(result.0.is_err());
}

#[tokio::test]
async fn stalled_exchange_can_be_cancelled_by_its_supervisor() {
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        test_peer::run(Case::Stalled),
    )
    .await;
    assert!(result.is_err());
}

#[test]
fn advertised_agent_values_are_not_display_names() {
    let options = test_peer::options("worker");
    assert_eq!(
        (
            super::selection::advertised(&options, "worker").is_ok(),
            super::selection::advertised(&options, "Display label").is_err()
        ),
        (true, true)
    );
}
