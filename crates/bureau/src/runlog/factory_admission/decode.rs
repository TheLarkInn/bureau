use std::io::{self, BufReader, Read, Seek as _, SeekFrom};

use bureau_plugin::Sha256;

use super::super::{Event, EventKind, RunState, replay};
use super::source::{RunSource, Version};

pub(super) enum Prepared {
    Changed,
    Bound(io::Result<Option<Box<RunState>>>),
}

impl Prepared {
    fn new(state: io::Result<Option<RunState>>) -> Self {
        Self::Bound(state.map(|state| state.map(Box::new)))
    }

    pub(super) const fn bound(&self) -> bool {
        matches!(self, Self::Bound(_))
    }

    pub(super) const fn pipeline(&self) -> bool {
        matches!(self, Self::Bound(Ok(Some(_))))
    }

    pub(super) fn state(self) -> io::Result<Option<RunState>> {
        match self {
            Self::Bound(state) => state.map(|state| state.map(|state| *state)),
            Self::Changed => Err(io::Error::other("run changed during replay preparation")),
        }
    }
}

fn event(bytes: &[u8]) -> io::Result<Option<Event>> {
    let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str(text) {
        Ok(event) => Ok(Some(event)),
        Err(_) if !bytes.ends_with(b"\n") => Ok(None),
        Err(error) => Err(io::Error::other(error)),
    }
}

struct Fold {
    state: Option<RunState>,
    cloud_only: bool,
    seen: bool,
    error: Option<io::Error>,
}

impl Fold {
    const fn new(state: Option<RunState>) -> Self {
        Self {
            state,
            cloud_only: true,
            seen: false,
            error: None,
        }
    }

    fn apply(&mut self, event: Event) {
        self.seen = true;
        self.cloud_only &= event.kind == EventKind::GitHubCloud;
        if let Some(state) = &mut self.state {
            state.apply(&event);
        } else {
            self.state = replay([event]);
        }
    }

    fn line(&mut self, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        match event(bytes) {
            Ok(Some(event)) => self.apply(event),
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
    }

    fn finish(self) -> io::Result<Option<RunState>> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.state.is_some() || (self.cloud_only && self.seen) {
            return Ok(self.state);
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "log has no valid run_started event",
        ))
    }
}

struct Hashed<R> {
    reader: R,
    hash: Sha256,
    len: u64,
}

impl<R: Read> Read for Hashed<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(bytes)?;
        self.hash.update(&bytes[..count]);
        self.len += count as u64;
        Ok(count)
    }
}

fn fold(reader: &mut impl std::io::BufRead, state: Option<RunState>) -> io::Result<Fold> {
    let mut fold = Fold::new(state);
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? != 0 {
        fold.line(&line);
        line.clear();
    }
    Ok(fold)
}

fn read_error(error: io::Error) -> Prepared {
    // Path/read races require a fresh fenced source, not a claim of corrupt history.
    match error.kind() {
        io::ErrorKind::NotFound
        | io::ErrorKind::Interrupted
        | io::ErrorKind::UnexpectedEof
        | io::ErrorKind::PermissionDenied => Prepared::Changed,
        _ => Prepared::Bound(Err(error)),
    }
}

#[derive(Clone, Copy)]
struct Range {
    offset: u64,
    len: u64,
    digest: [u8; 32],
}

fn read(source: &RunSource, range: Range, state: Option<RunState>) -> io::Result<Prepared> {
    let Some(mut file) = source.open()? else {
        return Ok(Prepared::Changed);
    };
    file.seek(SeekFrom::Start(range.offset))?;
    let hashed = Hashed {
        reader: file.take(range.len),
        hash: Sha256::default(),
        len: 0,
    };
    let mut reader = BufReader::new(hashed);
    let state = fold(&mut reader, state)?;
    let hashed = reader.into_inner();
    if hashed.len != range.len || hashed.hash.finish() != range.digest {
        return Ok(Prepared::Changed);
    }
    Ok(Prepared::new(state.finish()))
}

const fn full(version: Version) -> Range {
    Range {
        offset: 0,
        len: version.len,
        digest: version.digest,
    }
}

pub(super) fn prepare(source: &RunSource, previous: Option<(RunSource, Prepared)>) -> Prepared {
    let Some(version) = source.version() else {
        return Prepared::Bound(Err(io::Error::new(
            io::ErrorKind::NotFound,
            "run has no events log",
        )));
    };
    let mut range = full(version);
    let mut state = None;
    if let Some((before, prior)) = previous {
        if before.same(source) && prior.bound() {
            return prior;
        }
        if let Some(appended) = before.appended(source)
            && let Prepared::Bound(Ok(Some(prior))) = prior
        {
            range = Range {
                offset: appended.base.len,
                len: version.len - appended.base.len,
                digest: appended.digest,
            };
            state = Some(*prior);
        }
    }
    read(source, range, state).unwrap_or_else(read_error)
}
