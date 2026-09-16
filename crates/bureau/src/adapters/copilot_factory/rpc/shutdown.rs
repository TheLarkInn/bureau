use tokio::sync::watch;
use tokio::task::AbortHandle;

use super::Error;

pub(super) struct Shutdown {
    status: watch::Sender<Option<Error>>,
    reader: AbortHandle,
}

impl Shutdown {
    pub(super) const fn new(status: watch::Sender<Option<Error>>, reader: AbortHandle) -> Self {
        Self { status, reader }
    }

    pub(super) fn fail(&self, error: Error) {
        let _ = self.status.send_replace(Some(error));
    }
}

impl Drop for Shutdown {
    fn drop(&mut self) {
        self.reader.abort();
        let unrecorded = self.status.borrow().is_none();
        if unrecorded {
            self.fail(Error::Closed(
                "dispatcher task stopped without a recorded terminal cause",
            ));
        }
    }
}

pub(super) async fn closed(mut status: watch::Receiver<Option<Error>>) -> Error {
    let _ = status.wait_for(Option::is_some).await;
    status.borrow().clone().unwrap_or(Error::Closed(
        "dispatcher task ended before recording a terminal cause",
    ))
}
