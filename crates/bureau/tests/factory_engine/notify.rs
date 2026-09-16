use std::future::{Future, poll_fn};
use std::os::linux::net::SocketAddrExt as _;
use std::pin::pin;
use std::task::Poll;

use bureau::engine::RunOutcome;
use tokio::io::AsyncReadExt as _;
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

use super::fixture::Fixture;

pub fn listener(fixture: &Fixture, purpose: &str) -> UnixListener {
    let name = format!("{}-{purpose}", fixture.plan.run_id);
    let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())
        .expect("fixture address");
    let listener =
        std::os::unix::net::UnixListener::bind_addr(&address).expect("owned notification listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    std::fs::write(fixture.root.join(purpose), name).expect("fixture notification identity");
    UnixListener::from_std(listener).expect("asynchronous listener")
}

pub async fn first<L: Future, R: Future>(left: L, right: R) -> Result<L::Output, R::Output> {
    let mut left = pin!(left);
    let mut right = pin!(right);
    poll_fn(|context| match left.as_mut().poll(context) {
        Poll::Ready(value) => Poll::Ready(Ok(value)),
        Poll::Pending => right.as_mut().poll(context).map(Err),
    })
    .await
}

pub async fn admitted(listener: &UnixListener, task: &mut JoinHandle<RunOutcome>) -> bool {
    matches!(first(listener.accept(), task).await, Ok(Ok(_)))
}

pub async fn closed(listener: UnixListener) {
    let listener = listener.into_std().expect("nonblocking owned listener");
    let (stream, _) = listener
        .accept()
        .expect("descendant connected before the engine returned");
    stream
        .set_nonblocking(true)
        .expect("nonblocking descendant stream");
    let mut stream = UnixStream::from_std(stream).expect("asynchronous descendant stream");
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .await
        .expect("descendant termination notification");
}
