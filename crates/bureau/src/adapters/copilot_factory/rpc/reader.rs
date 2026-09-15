use serde_json::Value;
use tokio::io::AsyncBufRead;
use tokio::sync::mpsc;

use super::super::wire::read_frame;
use super::Error;

pub(super) type Incoming = Result<Value, Error>;

async fn read_one(
    reader: &mut (impl AsyncBufRead + Unpin),
    incoming: &mpsc::Sender<Incoming>,
) -> bool {
    let frame = read_frame(reader).await.map_err(Error::transport);
    let healthy = frame.is_ok();
    incoming.send(frame).await.is_ok() && healthy
}

pub(super) async fn run(mut reader: impl AsyncBufRead + Unpin, incoming: mpsc::Sender<Incoming>) {
    // A frame read may have consumed bytes before yielding. Only terminating the
    // whole connection may cancel it; outbound traffic never competes with it.
    while read_one(&mut reader, &incoming).await {}
}
