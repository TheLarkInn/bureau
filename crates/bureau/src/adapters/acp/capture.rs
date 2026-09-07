//! Scrub decoded content before capture or streaming it into the run log.

use std::io::{self, Write};

use crate::process::{ScrubWriter, Secret, SharedLog};

const TAIL_LIMIT: usize = 256 * 1024;

pub(super) struct Capture {
    pending: Vec<u8>,
    tail: Vec<u8>,
    log: Option<SharedLog>,
}

impl Capture {
    pub(super) fn scrubbed(log: Option<SharedLog>, secrets: &[Secret]) -> ScrubWriter<Self> {
        ScrubWriter::new(
            Self {
                pending: Vec::new(),
                tail: Vec::new(),
                log,
            },
            secrets,
        )
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.tail
    }

    fn forward(&mut self, take: usize) -> io::Result<()> {
        let bytes = &self.pending[..take];
        if let Some(log) = &self.log {
            log.lock()
                .map_err(|_| io::Error::other("ACP log lock poisoned"))?
                .write_all(bytes)?;
        }
        self.tail.extend_from_slice(bytes);
        self.tail
            .drain(..self.tail.len().saturating_sub(TAIL_LIMIT));
        self.pending.drain(..take);
        Ok(())
    }
}

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        let take = crate::process::complete_prefix(&self.pending);
        self.forward(take)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.forward(self.pending.len())
    }
}

pub(super) fn scrub_error(message: &str, secrets: &[Secret]) -> io::Result<String> {
    let mut writer = ScrubWriter::new(Vec::new(), secrets);
    writer.write_all(message.as_bytes())?;
    Ok(String::from_utf8_lossy(&writer.finish()?).into_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::process::shared_log;

    #[derive(Clone, Default)]
    struct Output(Arc<Mutex<Vec<u8>>>);

    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("output").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn decoded_content_is_scrubbed_before_capture_and_streaming() {
        let output = Output::default();
        let mut writer = Capture::scrubbed(
            Some(shared_log(output.clone())),
            &[Secret::new("secret-token")],
        );
        for text in ["prefix secret-", "token \u{20ac} suffix"] {
            writer.write_all(text.as_bytes()).expect("content");
        }
        let capture = writer.finish().expect("finish");
        let logged = output.0.lock().expect("output").clone();
        let expected = "prefix [REDACTED] \u{20ac} suffix".as_bytes();
        assert_eq!((capture.bytes(), logged.as_slice()), (expected, expected));
    }

    #[test]
    fn fallback_capture_keeps_only_a_bounded_tail() {
        let mut writer = Capture::scrubbed(None, &[]);
        writer
            .write_all(&vec![b'x'; TAIL_LIMIT + 20])
            .expect("content");
        assert_eq!(writer.finish().expect("finish").bytes().len(), TAIL_LIMIT);
    }
}
