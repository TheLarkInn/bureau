use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

use super::super::super::wire::{read_frame, write_frame};
use super::super::{CallbackRequest, Client, Error, Handler, RpcFault};

pub(super) type Reader = BufReader<ReadHalf<DuplexStream>>;
pub(super) type Writer = WriteHalf<DuplexStream>;

pub(super) async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("in-memory JSON-RPC operation stalled")
}

pub(super) fn response(request: &Value, result: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": request["id"], "result": result})
}

pub(super) fn running() -> Value {
    json!({"runId": "factory-17", "status": "running"})
}

pub(super) async fn encoded(messages: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for message in messages {
        write_frame(&mut bytes, message)
            .await
            .expect("encode frame");
    }
    bytes
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Event {
    Notification(String, Value),
    Response(String, Value, Result<Value, RpcFault>),
}

#[derive(Clone, Default)]
pub(super) struct Record(Arc<Mutex<Vec<Event>>>);

impl Record {
    pub(super) fn events(&self) -> Vec<Event> {
        self.0.lock().expect("observer log").clone()
    }

    fn append(&self, event: Event) {
        self.0.lock().expect("observer log").push(event);
    }
}

impl Handler for Record {
    fn notification(&mut self, method: &str, params: &Value) -> io::Result<Vec<CallbackRequest>> {
        self.append(Event::Notification(method.to_owned(), params.clone()));
        Ok(Vec::new())
    }

    fn response(
        &mut self,
        method: &str,
        params: &Value,
        result: &Result<Value, RpcFault>,
    ) -> io::Result<()> {
        self.append(Event::Response(
            method.to_owned(),
            params.clone(),
            result.clone(),
        ));
        Ok(())
    }
}

pub(super) struct Peer {
    reader: Reader,
    writer: Writer,
}

impl Peer {
    pub(super) async fn read(&mut self) -> Value {
        bounded(read_frame(&mut self.reader))
            .await
            .expect("read client frame")
    }

    pub(super) async fn send(&mut self, message: &Value) {
        bounded(write_frame(&mut self.writer, message))
            .await
            .expect("write peer frame");
    }

    pub(super) async fn reply(&mut self, request: &Value, result: &Value) {
        self.send(&response(request, result)).await;
    }

    pub(super) async fn send_bytes(&mut self, bytes: &[u8]) {
        bounded(self.writer.write_all(bytes))
            .await
            .expect("write frame fragment");
    }

    pub(super) async fn eof(&mut self) -> usize {
        bounded(self.reader.read(&mut [0; 1]))
            .await
            .expect("read peer EOF")
    }

    pub(super) async fn until_sent(
        &mut self,
        call: Pin<&mut impl Future<Output = Result<Value, Error>>>,
    ) -> Value {
        bounded(async {
            tokio::select! {
                result = call => panic!("call completed before a reply: {result:?}"),
                request = self.read() => request,
            }
        })
        .await
    }

    pub(super) async fn read_many(&mut self, count: usize) -> Vec<Value> {
        let mut requests = Vec::new();
        for _ in 0..count {
            requests.push(self.read().await);
        }
        requests
    }

    pub(super) async fn reply_reversed(&mut self, requests: &[Value]) {
        for request in requests.iter().rev() {
            self.reply(request, &request["method"]).await;
        }
    }
}

pub(super) fn streams() -> (Reader, Writer, Peer) {
    let (client, peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let (peer_reader, peer_writer) = tokio::io::split(peer);
    let peer = Peer {
        reader: BufReader::new(peer_reader),
        writer: peer_writer,
    };
    (BufReader::new(reader), writer, peer)
}

pub(super) fn connect(handler: impl Handler) -> (Client, Peer) {
    let (reader, writer, peer) = streams();
    (Client::start(reader, writer, handler), peer)
}

pub(super) async fn exchange(
    client: &Client,
    peer: &mut Peer,
    method: &str,
    params: Value,
    reply: &Value,
) -> Result<Value, Error> {
    let server = async {
        let request = peer.read().await;
        peer.reply(&request, reply).await;
    };
    let (result, ()) = bounded(async { tokio::join!(client.call(method, params), server) }).await;
    result
}
