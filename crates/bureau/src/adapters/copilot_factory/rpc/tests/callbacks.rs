use serde_json::{Value, json};

use super::super::{CallbackRequest, Client, Error, RpcFault};
use super::callback_support::{
    HANDLE, event, fail_acknowledgement, fenced_callback, generated, queued, rejection,
    serve_callback, serve_many,
};
use super::support::{Event, Peer, Record, bounded, exchange, running};

fn assert_observer(error: &Error, expected: &str) {
    match error {
        Error::Observer {
            callback,
            method,
            source,
        } => {
            let actual = (*callback, method.as_str(), source.to_string());
            assert_eq!(
                actual,
                (expected, HANDLE, "callback observer failed".to_owned())
            );
        }
        other => panic!("expected callback observer failure, got {other:?}"),
    }
}

async fn callback_roundtrip(
    record: &Record,
    acknowledgement: &Result<Value, RpcFault>,
) -> (Value, Vec<Value>) {
    let (client, mut peer) = queued(record, vec![rejection()], None);
    let server = serve_callback(&mut peer, acknowledgement);
    let (result, callbacks) =
        bounded(async { tokio::join!(client.call("factory.run", json!({})), server) }).await;
    (result.expect("initial response"), callbacks)
}

fn request_with_params(params: Value) -> CallbackRequest {
    CallbackRequest {
        method: HANDLE.to_owned(),
        params,
    }
}

async fn multiple_callbacks(record: &Record) -> Vec<Value> {
    let requests = vec![
        request_with_params(json!({"index": 1})),
        request_with_params(json!({"index": 2})),
    ];
    let (client, mut peer) = queued(record, requests, None);
    let server = serve_many(&mut peer, 2);
    let (result, callbacks) =
        bounded(async { tokio::join!(client.call("factory.run", json!({})), server) }).await;
    result.expect("initial response");
    callbacks
}

async fn acknowledge_after_abandonment(client: &Client, peer: &mut Peer) {
    let mut abandoned = Box::pin(client.call("factory.run", json!({})));
    peer.until_sent(abandoned.as_mut()).await;
    drop(abandoned);
    let requests = generated(peer, 1).await;
    peer.reply(&requests[0], &json!(true)).await;
}

#[tokio::test]
async fn permission_notification_generates_a_new_request_before_initial_reply() {
    let record = Record::default();
    let acknowledgement = Ok(json!(true));
    let (result, requests) = callback_roundtrip(&record, &acknowledgement).await;
    let rejection = rejection();
    let expected = json!({"jsonrpc": "2.0", "id": 2, "method": HANDLE, "params": rejection.params});
    let notification = Event::Notification("session.event".to_owned(), event()["params"].clone());
    let response = Event::Response(HANDLE.to_owned(), rejection.params, acknowledgement);
    let events = record.events();
    assert_eq!(
        (result, requests, events.first(), events.get(1)),
        (
            running(),
            vec![expected],
            Some(&notification),
            Some(&response)
        )
    );
}

#[tokio::test]
async fn callbacks_share_the_id_sequence_and_observe_out_of_order_acknowledgements() {
    let record = Record::default();
    let requests = multiple_callbacks(&record).await;
    let ids: Vec<_> = requests
        .iter()
        .map(|request| request["id"].clone())
        .collect();
    let events = record.events();
    let second = Event::Response(
        HANDLE.to_owned(),
        json!({"index": 2}),
        Ok(json!({"index": 2})),
    );
    let first = Event::Response(
        HANDLE.to_owned(),
        json!({"index": 1}),
        Ok(json!({"index": 1})),
    );
    assert_eq!(
        (ids, events.get(1), events.get(2)),
        (vec![json!(2), json!(3)], Some(&second), Some(&first))
    );
}

#[tokio::test]
async fn structured_remote_callback_fault_reaches_the_observer_without_a_caller() {
    let record = Record::default();
    let fault = RpcFault {
        code: -32001,
        message: "Permission acknowledgement failed".to_owned(),
        data: Some(json!({"code": "NOT_PENDING"})),
    };
    let (result, _) = callback_roundtrip(&record, &Err(fault.clone())).await;
    let expected = Event::Response(HANDLE.to_owned(), rejection().params, Err(fault));
    assert_eq!(
        (result, record.events().get(1)),
        (running(), Some(&expected))
    );
}

#[tokio::test]
async fn callback_observer_can_fail_closed_on_a_false_acknowledgement() {
    let record = Record::default();
    let (client, mut peer) = queued(&record, vec![rejection()], Some("response"));
    let server = fail_acknowledgement(&mut peer);
    let (result, ()) =
        bounded(async { tokio::join!(client.call("factory.run", json!({})), server) }).await;
    let error = result.expect_err("observer rejected the acknowledgement");
    assert_observer(&error, "response");
    let closed = bounded(client.closed()).await;
    assert_eq!(error.to_string(), closed.to_string());
}

#[tokio::test]
async fn notification_generated_request_rechecks_the_write_fence() {
    let record = Record::default();
    let (client, mut peer) = queued(&record, vec![rejection()], Some("before_request"));
    let server = fenced_callback(&mut peer);
    let (result, eof) =
        bounded(async { tokio::join!(client.call("factory.run", json!({})), server) }).await;
    assert_observer(&result.expect_err("callback fenced"), "before_request");
    assert_eq!(eof, 0);
}

#[tokio::test]
async fn callback_acknowledgement_is_observed_after_the_original_call_is_dropped() {
    let record = Record::default();
    let (client, mut peer) = queued(&record, vec![rejection()], None);
    acknowledge_after_abandonment(&client, &mut peer).await;
    exchange(&client, &mut peer, "factory.getRun", json!({}), &running())
        .await
        .expect("later reply");
    let expected = Event::Response(HANDLE.to_owned(), rejection().params, Ok(json!(true)));
    assert_eq!(record.events().get(1), Some(&expected));
}
