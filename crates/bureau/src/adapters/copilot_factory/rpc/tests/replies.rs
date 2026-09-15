use serde_json::{Value, json};

use super::super::dispatch::next_id;
use super::super::{Error, RpcFault};
use super::support::{Event, Peer, Record, bounded, connect, exchange};

fn remote(error: Error) -> RpcFault {
    match error {
        Error::Remote(fault) => fault,
        other => panic!("expected structured remote fault, got {other:?}"),
    }
}

async fn reject_reverse(id: &Value) -> Value {
    let (_client, mut peer) = connect(());
    let request = json!({"jsonrpc": "2.0", "id": id, "method": "approve.everything"});
    peer.send(&request).await;
    peer.read().await
}

async fn reply_fault(peer: &mut Peer, fault: &RpcFault) {
    let request = peer.read().await;
    peer.send(&json!({"jsonrpc": "2.0", "id": request["id"], "error": fault}))
        .await;
}

async fn reply_null(peer: &mut Peer) -> Value {
    let request = peer.read().await;
    peer.reply(&request, &Value::Null).await;
    request
}

async fn fault_roundtrip(data: Option<Value>) -> (RpcFault, Vec<Event>) {
    let record = Record::default();
    let (client, mut peer) = connect(record.clone());
    let fault = RpcFault {
        code: -32001,
        message: "Factory failed".to_owned(),
        data,
    };
    let server = reply_fault(&mut peer, &fault);
    let (result, ()) =
        bounded(async { tokio::join!(client.call("factory.run", json!({})), server) }).await;
    exchange(
        &client,
        &mut peer,
        "factory.getRun",
        Value::Null,
        &Value::Null,
    )
    .await
    .expect("connection survives a remote error");
    (remote(result.expect_err("remote error")), record.events())
}

#[tokio::test]
async fn null_is_a_present_success_result() {
    let (client, mut peer) = connect(());
    let result = exchange(
        &client,
        &mut peer,
        "factory.getRun",
        json!({}),
        &Value::Null,
    )
    .await;
    assert_eq!(result.expect("null reply"), Value::Null);
}

#[tokio::test]
async fn null_parameters_are_omitted_without_inventing_an_empty_object() {
    let (client, mut peer) = connect(());
    let server = reply_null(&mut peer);
    let (result, request) =
        bounded(async { tokio::join!(client.call("ping", Value::Null), server) }).await;
    let expected = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    assert_eq!(
        (result.expect("ping reply"), request),
        (Value::Null, expected)
    );
}

#[tokio::test]
async fn runtime_shutdown_explicitly_omits_params_and_returns_null() {
    let (client, mut peer) = connect(());
    let server = reply_null(&mut peer);
    let (result, request) =
        bounded(async { tokio::join!(client.call_without_params("runtime.shutdown"), server) })
            .await;
    let expected = json!({"jsonrpc": "2.0", "id": 1, "method": "runtime.shutdown"});
    assert_eq!(
        (result.expect("shutdown reply"), request),
        (Value::Null, expected)
    );
}

#[tokio::test]
async fn default_reverse_handler_preserves_string_and_numeric_ids_and_denies() {
    for id in [json!("request:9"), json!(7), json!(-7), json!(7.5)] {
        let reply = reject_reverse(&id).await;
        let expected = json!({
            "jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"}
        });
        assert_eq!(reply, expected);
    }
}

#[tokio::test]
async fn remote_errors_preserve_code_message_and_missing_null_or_structured_data() {
    for data in [
        None,
        Some(Value::Null),
        Some(json!({"code": "FACTORY_BUSY", "details": [1, null]})),
    ] {
        let (fault, events) = fault_roundtrip(data.clone()).await;
        let expected = RpcFault {
            code: -32001,
            message: "Factory failed".to_owned(),
            data,
        };
        let recorded = Event::Response("factory.run".to_owned(), json!({}), Err(expected.clone()));
        assert_eq!((fault, events.first()), (expected, Some(&recorded)));
    }
}

#[test]
fn request_ids_stop_at_the_integer_limit_without_wrapping() {
    let mut last = u64::MAX - 1;
    let assigned = next_id(&mut last).expect("last available id");
    let exhausted = matches!(next_id(&mut last), Err(Error::IdExhausted));
    assert_eq!((assigned, last, exhausted), (u64::MAX, u64::MAX, true));
}
