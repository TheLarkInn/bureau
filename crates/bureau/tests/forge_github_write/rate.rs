//! GitHub rate-limit response coverage.

use bureau::forge::{Error, Forge as _};

use super::TestServer;

#[tokio::test]
async fn rate_limit_preserves_retry_delay() {
    let body = r#"{"message":"rate limit exceeded"}"#;
    let reply = format!(
        "HTTP/1.1 429 Too Many Requests\r\nretry-after: 17\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let server = TestServer::start(move |_| vec![reply]);
    let error = server
        .forge()
        .query("o/r", "is:issue")
        .await
        .expect_err("rate limited");
    let Error::RateLimited {
        retry_after_secs,
        message,
    } = error
    else {
        panic!("expected rate limit")
    };
    assert_eq!(
        (retry_after_secs, message.contains("rate limit")),
        (Some(17), true)
    );
}

#[tokio::test]
async fn secondary_limit_body_stops_requests_without_headers() {
    let body = r#"{"message":"You have exceeded a secondary rate limit."}"#;
    let reply = format!(
        "HTTP/1.1 403 Forbidden\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let server = TestServer::start(move |_| vec![reply]);
    let error = server
        .forge()
        .query("o/r", "is:issue")
        .await
        .expect_err("secondary limit");
    assert!(matches!(
        error,
        Error::RateLimited {
            retry_after_secs: None,
            ..
        }
    ));
}
