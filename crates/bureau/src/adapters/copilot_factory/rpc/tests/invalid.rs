use serde_json::{Value, json};

use super::super::{Client, Error};
use super::support::{Peer, Record, bounded, connect, exchange};

fn invalid_ids() -> Vec<Value> {
    [
        Value::Null,
        json!(true),
        json!("1"),
        json!(1.0),
        json!(-1),
        json!(900),
        json!(0),
    ]
    .into_iter()
    .map(|id| json!({"jsonrpc": "2.0", "id": id, "result": null}))
    .collect()
}

fn invalid_envelopes() -> Vec<Value> {
    vec![
        json!([]),
        json!({"id": 1, "result": null}),
        json!({"jsonrpc": "1.0", "id": 1, "result": null}),
        json!({"jsonrpc": "2.0", "id": 1}),
        json!({"jsonrpc": "2.0", "result": null}),
        json!({"jsonrpc": "2.0", "id": 1, "result": null, "error": null}),
        json!({"jsonrpc": "2.0", "method": 1, "id": 1}),
        json!({"jsonrpc": "2.0", "method": "resolve", "result": null}),
        json!({"jsonrpc": "2.0", "method": "resolve", "id": false}),
        json!({"jsonrpc": "2.0", "method": "resolve", "params": null}),
        json!({"jsonrpc": "2.0", "method": "resolve", "params": 5}),
        json!({"jsonrpc": "2.0", "id": 1, "result": null, "params": {}}),
    ]
}

fn invalid_faults() -> Vec<Value> {
    [
        json!({"code": "1", "message": "bad"}),
        json!({"code": 1}),
        json!({"code": 1, "message": null}),
        Value::Null,
    ]
    .into_iter()
    .map(|error| json!({"jsonrpc": "2.0", "id": 1, "error": error}))
    .collect()
}

fn assert_protocol(errors: (Error, Error, Error)) {
    let (first, closed, later) = errors;
    assert!(matches!(first, Error::Protocol(_)), "{first}");
    let expected = (first.to_string(), first.to_string());
    assert_eq!((closed.to_string(), later.to_string()), expected);
}

async fn closed_errors(client: &Client, first: Error) -> (Error, Error, Error) {
    let closed = bounded(client.closed()).await;
    let later = bounded(client.call("later", json!({})))
        .await
        .expect_err("closed");
    (first, closed, later)
}

async fn invalid_reply(peer: &mut Peer, envelope: &Value) {
    peer.read().await;
    peer.send(envelope).await;
}

async fn reject(envelope: &Value) -> (Error, Error, Error) {
    let (client, mut peer) = connect(());
    let server = invalid_reply(&mut peer, envelope);
    let (result, ()) =
        bounded(async { tokio::join!(client.call("factory.run", json!({})), server) }).await;
    closed_errors(&client, result.expect_err("invalid envelope")).await
}

async fn duplicate_response(record: &Record) -> (Error, Error, Error) {
    let (client, mut peer) = connect(record.clone());
    exchange(&client, &mut peer, "first", json!({}), &Value::Null)
        .await
        .expect("first");
    let duplicate = json!({"jsonrpc": "2.0", "id": 1, "result": "duplicate"});
    let server = invalid_reply(&mut peer, &duplicate);
    let (result, ()) =
        bounded(async { tokio::join!(client.call("second", json!({})), server) }).await;
    closed_errors(&client, result.expect_err("duplicate")).await
}

#[tokio::test]
async fn invalid_or_unknown_response_ids_fail_the_connection_with_a_retained_cause() {
    for envelope in invalid_ids() {
        assert_protocol(reject(&envelope).await);
    }
}

#[tokio::test]
async fn malformed_envelopes_and_faults_fail_closed_instead_of_hanging() {
    for envelope in invalid_envelopes().into_iter().chain(invalid_faults()) {
        assert_protocol(reject(&envelope).await);
    }
}

#[tokio::test]
async fn duplicate_response_fails_other_pending_calls_without_observing_it_twice() {
    let record = Record::default();
    assert_protocol(duplicate_response(&record).await);
    assert_eq!(record.events().len(), 1);
}
