use agent_client_protocol::schema::v1::NewSessionRequest;
use agent_client_protocol::{ByteStreams, Result};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, DuplexStream};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

use super::{events::Events, exchange::exchange};

async fn reply(server: DuplexStream, fixture: &str) {
    let (input, mut output) = tokio::io::split(server);
    let mut input = BufReader::new(input);
    let mut request = String::new();
    input.read_line(&mut request).await.expect("initialize");
    let request: serde_json::Value = serde_json::from_str(&request).expect("request JSON");
    let response = fixture.replace("$ID", &request["id"].to_string()) + "\n";
    output.write_all(response.as_bytes()).await.expect("reply");
    closed(input, fixture == "not JSON").await;
}

async fn closed(mut input: impl tokio::io::AsyncBufRead + Unpin, malformed: bool) {
    let mut next = String::new();
    input
        .read_line(&mut next)
        .await
        .expect("client response or EOF");
    assert_reply(&next, malformed);
}

fn assert_reply(reply: &str, malformed: bool) {
    if malformed {
        let reply: serde_json::Value = serde_json::from_str(reply).expect("parse error response");
        assert_eq!(reply["error"]["code"], -32700);
    } else {
        assert!(
            reply.is_empty(),
            "invalid initialize was not rejected: {reply}"
        );
    }
}

async fn handshake(fixture: &str) -> Result<()> {
    let (client, server) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(client);
    let client = exchange(
        ByteStreams::new(output.compat_write(), input.compat()),
        NewSessionRequest::new("/tmp"),
        "worker".to_owned(),
        String::new(),
        Events::new("claude", &[], None, None),
    );
    let (result, ()) = tokio::join!(client, reply(server, fixture));
    result
}

#[tokio::test]
async fn unsupported_version_and_malformed_wire_replies_fail() {
    // The official Agent normalizes initialize responses; invalid wire fixtures
    // must bypass that server-side normalization to exercise the real client.
    for fixture in [
        r#"{"jsonrpc":"2.0","id":$ID,"result":{"protocolVersion":99,"agentCapabilities":{}}}"#,
        r#"{"jsonrpc":"2.0","id":$ID,"result":{"agentCapabilities":{}}}"#,
        "not JSON",
    ] {
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), handshake(fixture))
            .await
            .expect("invalid initialize must not hang");
        assert!(result.is_err(), "{fixture}");
    }
}
