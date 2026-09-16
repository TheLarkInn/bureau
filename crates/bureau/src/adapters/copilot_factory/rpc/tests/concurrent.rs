use serde_json::{Value, json};

use super::super::{Client, Handler, RpcFault};
use super::support::{Event, Peer, Record, bounded, connect, exchange, running};

struct Resolve;

impl Handler for Resolve {
    fn request(&mut self, method: &str, params: &Value) -> Result<Value, RpcFault> {
        match method {
            "factory.resolve" => Ok(json!({"definition": params["name"]})),
            _ => ().request(method, params),
        }
    }
}

async fn notify_then_reply(peer: &mut Peer) {
    let request = peer.read().await;
    peer.send(&json!({"jsonrpc": "2.0", "method": "factory.started", "params": running()}))
        .await;
    peer.reply(&request, &running()).await;
}

async fn reverse_reply(peer: &mut Peer, id: &Value) -> Value {
    let reverse = json!({
        "jsonrpc": "2.0", "id": id, "method": "factory.resolve", "params": {"name": "review"}
    });
    peer.send(&reverse).await;
    peer.read().await
}

async fn resolve_then_reply(peer: &mut Peer, id: &Value) -> Value {
    let request = peer.read().await;
    let resolved = reverse_reply(peer, id).await;
    peer.reply(&request, &running()).await;
    resolved
}

async fn resolved_request(id: &Value) -> (Value, Value) {
    let (client, mut peer) = connect(Resolve);
    let server = resolve_then_reply(&mut peer, id);
    let (result, resolved) =
        bounded(async { tokio::join!(client.call("factory.run", json!({})), server) }).await;
    (result.expect("initial reply"), resolved)
}

async fn reply_controls(peer: &mut Peer) {
    let requests = peer.read_many(3).await;
    peer.reply_reversed(&requests).await;
}

async fn abandon_run(client: &Client, peer: &mut Peer) {
    let mut abandoned = Box::pin(client.call("factory.run", json!({"attempt": 2})));
    let request = peer.until_sent(abandoned.as_mut()).await;
    drop(abandoned);
    peer.reply(&request, &running()).await;
}

#[tokio::test]
async fn notification_precedes_initial_running_reply_and_caller_completion() {
    let record = Record::default();
    let (client, mut peer) = connect(record.clone());
    let server = notify_then_reply(&mut peer);
    let (result, ()) =
        bounded(async { tokio::join!(client.call("factory.run", json!({"attempt": 1})), server) })
            .await;
    let events = vec![
        Event::Notification("factory.started".to_owned(), running()),
        Event::Response(
            "factory.run".to_owned(),
            json!({"attempt": 1}),
            Ok(running()),
        ),
    ];
    assert_eq!(
        (result.expect("initial reply"), record.events()),
        (running(), events)
    );
}

#[tokio::test]
async fn reverse_resolve_is_serviced_while_the_initial_response_is_pending() {
    for id in [json!("resolve:7"), json!(1)] {
        let result = resolved_request(&id).await;
        let expected = json!({"jsonrpc": "2.0", "id": id, "result": {"definition": "review"}});
        assert_eq!(result, (running(), expected));
    }
}

#[tokio::test]
async fn pending_get_does_not_block_pause_cancel_or_out_of_order_replies() {
    let (client, mut peer) = connect(());
    let server = reply_controls(&mut peer);
    let (get, pause, cancel, ()) = bounded(async {
        tokio::join!(
            client.call("factory.getRun", json!({"runId": "factory-17"})),
            client.call("factory.pauseRun", json!({"runId": "factory-17"})),
            client.call("factory.cancelRun", json!({"runId": "factory-17"})),
            server
        )
    })
    .await;
    let actual = (
        get.expect("get"),
        pause.expect("pause"),
        cancel.expect("cancel"),
    );
    let expected = (
        json!("factory.getRun"),
        json!("factory.pauseRun"),
        json!("factory.cancelRun"),
    );
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn dropped_call_still_persists_its_response_before_the_next_caller_returns() {
    let record = Record::default();
    let (client, mut peer) = connect(record.clone());
    abandon_run(&client, &mut peer).await;
    exchange(&client, &mut peer, "factory.getRun", json!({}), &json!({}))
        .await
        .expect("second reply");
    let events = vec![
        Event::Response(
            "factory.run".to_owned(),
            json!({"attempt": 2}),
            Ok(running()),
        ),
        Event::Response("factory.getRun".to_owned(), json!({}), Ok(json!({}))),
    ];
    assert_eq!(record.events(), events);
}
