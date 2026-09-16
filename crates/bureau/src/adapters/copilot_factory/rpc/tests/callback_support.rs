use std::io;

use serde_json::{Value, json};

use super::super::{CallbackRequest, Client, Handler, RpcFault};
use super::support::{Peer, Record, connect, running};

pub(super) const HANDLE: &str = "session.permissions.handlePendingPermissionRequest";

pub(super) fn event() -> Value {
    json!({
        "jsonrpc": "2.0", "method": "session.event",
        "params": {
            "sessionId": "session-local",
            "event": {
                "type": "permission.requested",
                "data": {
                    "requestId": "permission-7",
                    "permissionRequest": {"kind": "shell"},
                    "resolvedByHook": false
                }
            }
        }
    })
}

pub(super) fn rejection() -> CallbackRequest {
    CallbackRequest {
        method: HANDLE.to_owned(),
        params: json!({
            "sessionId": "session-local",
            "requestId": "permission-7",
            "result": {"kind": "reject", "feedback": "Denied by Bureau policy."}
        }),
    }
}

struct Notifications {
    record: Record,
    requests: Vec<CallbackRequest>,
    failure: Option<&'static str>,
}

impl Notifications {
    fn check(&self, callback: &str, method: &str) -> io::Result<()> {
        if self.failure == Some(callback) && method == HANDLE {
            return Err(io::Error::other("callback observer failed"));
        }
        Ok(())
    }
}

impl Handler for Notifications {
    fn notification(&mut self, method: &str, params: &Value) -> io::Result<Vec<CallbackRequest>> {
        let _ = self.record.notification(method, params)?;
        Ok(std::mem::take(&mut self.requests))
    }

    fn response(
        &mut self,
        method: &str,
        params: &Value,
        result: &Result<Value, RpcFault>,
    ) -> io::Result<()> {
        self.record.response(method, params, result)?;
        self.check("response", method)
    }

    fn before_request(&mut self, method: &str, _params: &Value) -> io::Result<()> {
        self.check("before_request", method)
    }
}

pub(super) fn queued(
    record: &Record,
    requests: Vec<CallbackRequest>,
    failure: Option<&'static str>,
) -> (Client, Peer) {
    let handler = Notifications {
        record: record.clone(),
        requests,
        failure,
    };
    connect(handler)
}

pub(super) async fn generated(peer: &mut Peer, count: usize) -> Vec<Value> {
    peer.send(&event()).await;
    peer.read_many(count).await
}

pub(super) async fn acknowledge(
    peer: &mut Peer,
    request: &Value,
    result: &Result<Value, RpcFault>,
) {
    let message = match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": request["id"], "result": value}),
        Err(error) => json!({"jsonrpc": "2.0", "id": request["id"], "error": error}),
    };
    peer.send(&message).await;
}

async fn finish(
    peer: &mut Peer,
    initial: &Value,
    callback: &Value,
    acknowledgement: &Result<Value, RpcFault>,
) {
    acknowledge(peer, callback, acknowledgement).await;
    peer.reply(initial, &running()).await;
}

pub(super) async fn serve_callback(
    peer: &mut Peer,
    acknowledgement: &Result<Value, RpcFault>,
) -> Vec<Value> {
    let initial = peer.read().await;
    let callbacks = generated(peer, 1).await;
    finish(peer, &initial, &callbacks[0], acknowledgement).await;
    callbacks
}

async fn acknowledge_reversed(peer: &mut Peer, requests: &[Value]) {
    for request in requests.iter().rev() {
        peer.reply(request, &request["params"]).await;
    }
}

async fn finish_many(peer: &mut Peer, initial: &Value, callbacks: &[Value]) {
    acknowledge_reversed(peer, callbacks).await;
    peer.reply(initial, &running()).await;
}

pub(super) async fn serve_many(peer: &mut Peer, count: usize) -> Vec<Value> {
    let initial = peer.read().await;
    let callbacks = generated(peer, count).await;
    finish_many(peer, &initial, &callbacks).await;
    callbacks
}

pub(super) async fn fail_acknowledgement(peer: &mut Peer) {
    peer.read().await;
    let callbacks = generated(peer, 1).await;
    peer.reply(&callbacks[0], &json!(false)).await;
}

pub(super) async fn fenced_callback(peer: &mut Peer) -> usize {
    peer.read().await;
    peer.send(&event()).await;
    peer.eof().await
}
