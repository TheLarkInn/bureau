use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use serde_json::{Value, json};
use tokio::io::AsyncWrite;
use tokio::sync::oneshot;

use super::super::{CallbackRequest, Client, Error, Handler, RpcFault};
use super::support::{Peer, bounded, connect, running, streams};

struct Failure(&'static str);

impl Failure {
    fn check(&self, callback: &str) -> io::Result<()> {
        if self.0 == callback {
            return Err(io::Error::other("durable append failed"));
        }
        Ok(())
    }
}

impl Handler for Failure {
    fn notification(&mut self, _method: &str, _params: &Value) -> io::Result<Vec<CallbackRequest>> {
        self.check("notification")?;
        Ok(Vec::new())
    }

    fn response(
        &mut self,
        _method: &str,
        _params: &Value,
        _result: &Result<Value, RpcFault>,
    ) -> io::Result<()> {
        self.check("response")
    }

    fn before_request(&mut self, _method: &str, _params: &Value) -> io::Result<()> {
        self.check("before_request")
    }
}

struct FailedWriter;

impl AsyncWrite for FailedWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "fake writer disconnected",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct DropHandler(Option<oneshot::Sender<()>>);

impl Handler for DropHandler {}

impl Drop for DropHandler {
    fn drop(&mut self) {
        if let Some(signal) = self.0.take() {
            let _ = signal.send(());
        }
    }
}

fn observer(error: &Error) -> (&str, &io::Error) {
    match error {
        Error::Observer {
            callback, source, ..
        } => (callback, source.as_ref()),
        other => panic!("expected observer failure, got {other:?}"),
    }
}

fn assert_observer(errors: &[Error], callback: &str) {
    for error in errors {
        let (actual, source) = observer(error);
        let expected = (
            callback,
            "durable append failed".to_owned(),
            errors[0].to_string(),
        );
        assert_eq!((actual, source.to_string(), error.to_string()), expected);
    }
}

fn transport(error: &Error) -> &io::Error {
    match error {
        Error::Transport(source) => source.as_ref(),
        other => panic!("expected transport failure, got {other:?}"),
    }
}

async fn fail_message(peer: &mut Peer, requests: &[Value], callback: &str) {
    match callback {
        "notification" => {
            peer.send(&json!({"jsonrpc": "2.0", "method": "factory.started", "params": running()}))
                .await;
        }
        "response" => peer.reply(&requests[0], &running()).await,
        other => panic!("unexpected callback: {other}"),
    }
}

async fn fail_pending_peer(peer: &mut Peer, callback: &str) {
    let requests = peer.read_many(2).await;
    fail_message(peer, &requests, callback).await;
}

async fn dropped_streams(peer: &mut Peer, dropped: oneshot::Receiver<()>) -> usize {
    let eof = peer.eof().await;
    bounded(dropped).await.expect("handler was dropped");
    eof
}

async fn pending_failure(callback: &'static str) -> Vec<Error> {
    let (client, mut peer) = connect(Failure(callback));
    let server = fail_pending_peer(&mut peer, callback);
    let (first, second, ()) = bounded(async {
        tokio::join!(
            client.call("factory.run", json!({})),
            client.call("factory.getRun", json!({})),
            server
        )
    })
    .await;
    let closed = bounded(client.closed()).await;
    let later = bounded(client.call("later", json!({})))
        .await
        .expect_err("closed");
    vec![
        first.expect_err("first"),
        second.expect_err("second"),
        closed,
        later,
    ]
}

async fn disconnect(mut peer: Peer) {
    peer.read_many(2).await;
    drop(peer);
}

async fn peer_disconnected() -> Vec<Error> {
    let (client, peer) = connect(());
    let server = disconnect(peer);
    let (first, second, ()) = bounded(async {
        tokio::join!(
            client.call("first", json!({})),
            client.call("second", json!({})),
            server
        )
    })
    .await;
    let closed = bounded(client.closed()).await;
    let later = bounded(client.call("later", json!({})))
        .await
        .expect_err("closed");
    vec![
        first.expect_err("first"),
        second.expect_err("second"),
        closed,
        later,
    ]
}

#[tokio::test]
async fn observer_failure_is_retained_by_all_pending_and_future_callers() {
    for callback in ["notification", "response"] {
        assert_observer(&pending_failure(callback).await, callback);
    }
}

#[tokio::test]
async fn before_request_failure_prevents_any_bytes_from_being_written() {
    let (client, mut peer) = connect(Failure("before_request"));
    let error = bounded(client.call("factory.run", json!({})))
        .await
        .expect_err("fenced");
    let eof = peer.eof().await;
    assert_observer(&[error], "before_request");
    assert_eq!(eof, 0);
}

#[tokio::test]
async fn invalid_outbound_parameters_fail_before_writing() {
    let (client, mut peer) = connect(());
    let error = bounded(client.call("factory.run", json!(false)))
        .await
        .expect_err("invalid params");
    let eof = peer.eof().await;
    assert_eq!((matches!(error, Error::Protocol(_)), eof), (true, 0));
}

#[tokio::test]
async fn reader_eof_retains_the_transport_cause_for_pending_and_later_calls() {
    let errors = peer_disconnected().await;
    for error in &errors {
        let actual = (transport(error).kind(), error.to_string());
        assert_eq!(
            actual,
            (io::ErrorKind::UnexpectedEof, errors[0].to_string())
        );
    }
}

#[tokio::test]
async fn writer_failure_preserves_the_underlying_error() {
    let (reader, _unused_writer, _peer) = streams();
    let client = Client::start(reader, FailedWriter, ());
    let error = bounded(client.call("factory.run", json!({})))
        .await
        .expect_err("write failed");
    let closed = bounded(client.closed()).await;
    let actual = (transport(&error).kind(), transport(&closed).to_string());
    assert_eq!(
        actual,
        (
            io::ErrorKind::BrokenPipe,
            "fake writer disconnected".to_owned()
        )
    );
}

#[tokio::test]
async fn abandoned_call_persistence_failure_still_closes_the_connection() {
    let (client, mut peer) = connect(Failure("response"));
    let mut abandoned = Box::pin(client.call("factory.run", json!({})));
    let request = peer.until_sent(abandoned.as_mut()).await;
    drop(abandoned);
    peer.reply(&request, &running()).await;
    assert_observer(&[bounded(client.closed()).await], "response");
}

#[tokio::test]
async fn dropping_client_aborts_the_reader_and_dispatcher_and_drops_the_handler() {
    let (signal, dropped) = oneshot::channel();
    let (client, mut peer) = connect(DropHandler(Some(signal)));
    let mut pending = Box::pin(client.call("factory.getRun", json!({})));
    peer.until_sent(pending.as_mut()).await;
    drop(pending);
    drop(client);
    let eof = dropped_streams(&mut peer, dropped).await;
    assert_eq!(eof, 0);
}
