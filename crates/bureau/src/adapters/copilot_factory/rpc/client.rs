use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncWrite};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::dispatch::{Command, Dispatcher, Reply};
use super::shutdown::{self, Shutdown};
use super::{Error, Handler, reader};

const CHANNEL_CAPACITY: usize = 32;

/// One connection, with an owned reader and a single writer/dispatcher.
///
/// Share it through `Arc<Client>` when separately spawned callers need ownership.
/// Neither task retains the client, so dropping the last owner aborts both tasks.
pub struct Client {
    commands: mpsc::Sender<Command>,
    status: watch::Receiver<Option<Error>>,
    tasks: [JoinHandle<()>; 2],
}

impl Client {
    /// Waits for the retained terminal cause without consuming it.
    pub async fn closed(&self) -> Error {
        shutdown::closed(self.status.clone()).await
    }

    async fn submit(&self, command: Command) -> Result<(), Error> {
        match self.commands.send(command).await {
            Ok(()) => Ok(()),
            Err(_) => Err(self.closed().await),
        }
    }

    async fn reply(&self, response: Reply) -> Result<Value, Error> {
        match response.await {
            Ok(result) => result,
            Err(_) => Err(self.closed().await),
        }
    }

    /// Returns the first matching raw reply, without waiting for a run to finish.
    ///
    /// `params: null` omits the wire member; other parameters must be an object or
    /// array. Once enqueued, cancellation of this future does not retract the
    /// command or suppress the response observer.
    ///
    /// # Errors
    ///
    /// Returns the structured remote fault or the retained terminal cause.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, Error> {
        let (command, response) = Command::new(method, params);
        self.submit(command).await?;
        self.reply(response).await
    }

    /// Calls a method with no `params` member, preserving a null success result.
    ///
    /// # Errors
    ///
    /// Returns the structured remote fault or the retained terminal cause.
    pub async fn call_without_params(&self, method: &str) -> Result<Value, Error> {
        self.call(method, Value::Null).await
    }

    /// Starts the connection over streams owned by the local process supervisor.
    ///
    /// # Panics
    ///
    /// Panics when called outside a Tokio runtime.
    #[must_use]
    pub fn start<R, W, H>(reader: R, writer: W, handler: H) -> Self
    where
        R: AsyncBufRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
        H: Handler,
    {
        let (frames, incoming) = mpsc::channel(CHANNEL_CAPACITY);
        let (commands, outgoing) = mpsc::channel(CHANNEL_CAPACITY);
        let (terminal, status) = watch::channel(None);
        let reader = tokio::spawn(reader::run(reader, frames));
        let shutdown = Shutdown::new(terminal, reader.abort_handle());
        let dispatcher = Dispatcher::new(writer, handler);
        let dispatcher = tokio::spawn(dispatcher.run(incoming, outgoing, shutdown));
        Self {
            commands,
            status,
            tasks: [reader, dispatcher],
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
