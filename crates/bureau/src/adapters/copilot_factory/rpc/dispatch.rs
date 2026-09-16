use std::collections::BTreeMap;
use std::future::poll_fn;
use std::task::{Context, Poll};

use serde_json::Value;
use tokio::io::AsyncWrite;
use tokio::sync::{mpsc, oneshot};

use super::super::wire::write_frame;
use super::envelope::{self, Message};
use super::reader::Incoming;
use super::shutdown::Shutdown;
use super::{CallbackRequest, Error, Handler, RpcFault};

pub(super) type Reply = oneshot::Receiver<Result<Value, Error>>;

pub(super) struct Command {
    request: CallbackRequest,
    reply: Option<oneshot::Sender<Result<Value, Error>>>,
}

impl Command {
    pub(super) const fn callback(request: CallbackRequest) -> Self {
        Self {
            request,
            reply: None,
        }
    }

    pub(super) fn new(method: &str, params: Value) -> (Self, Reply) {
        let (reply, receiver) = oneshot::channel();
        let request = CallbackRequest {
            method: method.to_owned(),
            params,
        };
        let command = Self {
            request,
            reply: Some(reply),
        };
        (command, receiver)
    }

    fn complete(
        self,
        handler: &mut impl Handler,
        result: Result<Value, RpcFault>,
    ) -> Result<(), Error> {
        let request = self.request;
        handler
            .response(&request.method, &request.params, &result)
            .map_err(|error| Error::observer("response", &request.method, error))?;
        if let Some(reply) = self.reply {
            let _ = reply.send(result.map_err(Error::Remote));
        }
        Ok(())
    }
}

pub(super) fn next_id(last_id: &mut u64) -> Result<u64, Error> {
    let id = last_id.checked_add(1).ok_or(Error::IdExhausted)?;
    *last_id = id;
    Ok(id)
}

enum Input {
    Frame(Option<Incoming>),
    Command(Option<Command>),
}

impl Input {
    fn poll_frame(
        incoming: &mut mpsc::Receiver<Incoming>,
        commands: &mut mpsc::Receiver<Command>,
        context: &mut Context<'_>,
    ) -> Poll<Self> {
        if let Poll::Ready(frame) = incoming.poll_recv(context) {
            return Poll::Ready(Self::Frame(frame));
        }
        commands.poll_recv(context).map(Self::Command)
    }

    fn poll_command(
        incoming: &mut mpsc::Receiver<Incoming>,
        commands: &mut mpsc::Receiver<Command>,
        context: &mut Context<'_>,
    ) -> Poll<Self> {
        if let Poll::Ready(command) = commands.poll_recv(context) {
            return Poll::Ready(Self::Command(command));
        }
        incoming.poll_recv(context).map(Self::Frame)
    }

    async fn receive(
        incoming: &mut mpsc::Receiver<Incoming>,
        commands: &mut mpsc::Receiver<Command>,
        incoming_first: bool,
    ) -> Self {
        let poll = if incoming_first {
            Self::poll_frame
        } else {
            Self::poll_command
        };
        poll_fn(|context| poll(incoming, commands, context)).await
    }
}

pub(super) struct Dispatcher<W, H> {
    writer: W,
    handler: H,
    pending: BTreeMap<u64, Command>,
    last_id: u64,
    incoming_first: bool,
}

impl<W: AsyncWrite + Unpin, H: Handler> Dispatcher<W, H> {
    pub(super) const fn new(writer: W, handler: H) -> Self {
        Self {
            writer,
            handler,
            pending: BTreeMap::new(),
            last_id: 0,
            incoming_first: true,
        }
    }

    fn complete(&mut self, id: u64, result: Result<Value, RpcFault>) -> Result<(), Error> {
        self.pending
            .remove(&id)
            .ok_or_else(|| Error::Protocol(format!("unknown or duplicate response id {id}")))?
            .complete(&mut self.handler, result)
    }

    async fn outgoing(&mut self, command: Command) -> Result<(), Error> {
        let id = next_id(&mut self.last_id)?;
        let pending = self.pending.entry(id).or_insert(command);
        let request = &pending.request;
        let message = envelope::request(id, &request.method, &request.params)?;
        self.handler
            .before_request(&request.method, &request.params)
            .map_err(|error| Error::observer("before_request", &request.method, error))?;
        write_frame(&mut self.writer, &message)
            .await
            .map_err(Error::transport)
    }

    async fn notification(&mut self, method: &str, params: &Value) -> Result<(), Error> {
        let requests = self
            .handler
            .notification(method, params)
            .map_err(|error| Error::observer("notification", method, error))?;
        for request in requests {
            self.outgoing(Command::callback(request)).await?;
        }
        Ok(())
    }

    async fn reverse(&mut self, id: &Value, method: &str, params: &Value) -> Result<(), Error> {
        let response = envelope::reply(id, self.handler.request(method, params));
        write_frame(&mut self.writer, &response)
            .await
            .map_err(Error::transport)
    }

    async fn incoming_request(&mut self, message: Message) -> Result<(), Error> {
        let Message::Request { id, method, params } = message else {
            return Err(Error::Protocol("expected a reverse request".into()));
        };
        self.reverse(&id, &method, &params).await
    }

    async fn incoming_callback(&mut self, message: Message) -> Result<(), Error> {
        if let Message::Notification { method, params } = message {
            return self.notification(&method, &params).await;
        }
        self.incoming_request(message).await
    }

    async fn incoming(&mut self, value: &Value) -> Result<(), Error> {
        let message = envelope::parse(value)?;
        if let Message::Response { id, result } = message {
            return self.complete(id, result);
        }
        self.incoming_callback(message).await
    }

    async fn dispatch(&mut self, input: Input) -> Result<(), Error> {
        match input {
            Input::Frame(value) => {
                self.incoming(&value.ok_or(Error::Closed("reader task stopped"))??)
                    .await
            }
            Input::Command(command) => {
                self.outgoing(command.ok_or(Error::Closed("request channel closed"))?)
                    .await
            }
        }
    }

    async fn step(
        &mut self,
        incoming: &mut mpsc::Receiver<Incoming>,
        commands: &mut mpsc::Receiver<Command>,
    ) -> Result<(), Error> {
        let input = Input::receive(incoming, commands, self.incoming_first).await;
        // Alternate ready-channel priority without cancelling the frame reader.
        self.incoming_first = !self.incoming_first;
        self.dispatch(input).await
    }

    pub(super) async fn run(
        mut self,
        mut incoming: mpsc::Receiver<Incoming>,
        mut commands: mpsc::Receiver<Command>,
        shutdown: Shutdown,
    ) {
        let error = loop {
            if let Err(error) = self.step(&mut incoming, &mut commands).await {
                break error;
            }
        };
        shutdown.fail(error);
    }
}

#[cfg(test)]
mod tests;
