use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncRead, ReadBuf};
use tokio::sync::oneshot;

use super::super::Client;
use super::support::{Event, Peer, Record, bounded, encoded, response, running, streams};

struct Observed<R> {
    inner: R,
    target: usize,
    consumed: usize,
    signal: Option<oneshot::Sender<()>>,
}

impl<R> Observed<R> {
    fn new(inner: R, target: usize) -> (Self, oneshot::Receiver<()>) {
        let (signal, receiver) = oneshot::channel();
        let reader = Self {
            inner,
            target,
            consumed: 0,
            signal: Some(signal),
        };
        (reader, receiver)
    }

    fn signal(&mut self) {
        if let Some(signal) = self.signal.take() {
            let _ = signal.send(());
        }
    }

    fn consumed(&mut self, count: usize) {
        self.consumed = self.consumed.saturating_add(count);
        if self.consumed >= self.target {
            self.signal();
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Observed<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buffer.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(context, buffer);
        this.consumed(buffer.filled().len() - before);
        result
    }
}

impl<R: AsyncBufRead + Unpin> AsyncBufRead for Observed<R> {
    fn poll_fill_buf(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        Pin::new(&mut self.get_mut().inner).poll_fill_buf(context)
    }

    fn consume(self: Pin<&mut Self>, count: usize) {
        let this = self.get_mut();
        Pin::new(&mut this.inner).consume(count);
        this.consumed(count);
    }
}

async fn complete_fragments(peer: &mut Peer, request: &Value, suffix: &[u8]) {
    let messages = [
        json!({"jsonrpc": "2.0", "method": "factory.started", "params": running()}),
        response(request, &json!("cancelled")),
    ];
    let mut remainder = suffix.to_vec();
    remainder.extend(encoded(&messages).await);
    peer.send_bytes(&remainder).await;
}

struct Partial {
    client: Client,
    peer: Peer,
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    consumed: oneshot::Receiver<()>,
}

impl Partial {
    async fn new(record: Record) -> Self {
        let message = json!({"jsonrpc": "2.0", "id": 1, "result": running()});
        let mut prefix = encoded(&[message]).await;
        let target = prefix.len() - 1;
        let suffix = prefix.split_off(target);
        let (reader, writer, peer) = streams();
        let (reader, consumed) = Observed::new(reader, target);
        let client = Client::start(reader, writer, record);
        Self {
            client,
            peer,
            prefix,
            suffix,
            consumed,
        }
    }

    async fn interrupt(&mut self) {
        let mut abandoned = Box::pin(self.client.call("factory.run", json!({})));
        self.peer.until_sent(abandoned.as_mut()).await;
        self.peer.send_bytes(&self.prefix).await;
        bounded(&mut self.consumed)
            .await
            .expect("partial frame was consumed");
        drop(abandoned);
    }

    async fn finish(&mut self) -> Value {
        let mut cancel = Box::pin(self.client.call("factory.cancelRun", json!({})));
        let request = self.peer.until_sent(cancel.as_mut()).await;
        complete_fragments(&mut self.peer, &request, &self.suffix).await;
        bounded(cancel).await.expect("cancel reply")
    }

    async fn cancel_interrupted(&mut self) -> Value {
        self.interrupt().await;
        self.finish().await
    }
}

#[tokio::test]
async fn partial_frame_survives_call_cancellation_and_outbound_arrival_then_coalesced_frames() {
    let record = Record::default();
    let mut partial = Partial::new(record.clone()).await;
    let result = partial.cancel_interrupted().await;
    let events = vec![
        Event::Response("factory.run".to_owned(), json!({}), Ok(running())),
        Event::Notification("factory.started".to_owned(), running()),
        Event::Response(
            "factory.cancelRun".to_owned(),
            json!({}),
            Ok(json!("cancelled")),
        ),
    ];
    assert_eq!((result, record.events()), (json!("cancelled"), events));
}
