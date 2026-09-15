use std::fs::File;
use std::io::{self, Read as _};
use std::path::Path;

use bureau_plugin::Sha256;

use super::{Appended, Contents, Identity, Version, identity};

#[derive(Default)]
struct Summary {
    hash: Sha256,
    len: u64,
    has_newline: bool,
    ended: bool,
}

impl Summary {
    fn add(&mut self, bytes: &[u8]) {
        self.hash.update(bytes);
        self.len += bytes.len() as u64;
        self.has_newline |= bytes.contains(&b'\n');
        self.ended = bytes.ends_with(b"\n");
    }
}

struct Tail {
    base: Version,
    hash: Sha256,
    bytes: Option<Vec<u8>>,
    limit: usize,
}

impl Tail {
    fn new(base: Version, limit: usize) -> Self {
        Self {
            base,
            hash: Sha256::default(),
            bytes: Some(Vec::new()),
            limit,
        }
    }

    fn add(&mut self, bytes: &[u8]) {
        self.hash.update(bytes);
        if let Some(buffer) = &mut self.bytes {
            if bytes.len() <= self.limit.saturating_sub(buffer.len()) {
                buffer.reserve_exact(bytes.len());
                buffer.extend_from_slice(bytes);
            } else {
                self.bytes = None;
            }
        }
    }

    fn finish(self, remaining: &mut usize) -> Appended {
        *remaining -= self.bytes.as_ref().map_or(0, Vec::len);
        Appended {
            base: self.base,
            digest: self.hash.finish(),
            bytes: self.bytes,
        }
    }
}

fn scan(
    reader: &mut impl std::io::Read,
    summary: &mut Summary,
    tail: &mut Option<Tail>,
) -> io::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        summary.add(&buffer[..count]);
        if let Some(tail) = tail {
            tail.add(&buffer[..count]);
        }
    }
}

fn prefix(
    file: &mut File,
    current: Identity,
    prior: Option<&Contents>,
    summary: &mut Summary,
    remaining: usize,
) -> io::Result<Option<Tail>> {
    let Some(prior) = prior.filter(|prior| prior.version.identity == current) else {
        return Ok(None);
    };
    scan(&mut file.take(prior.version.len), summary, &mut None)?;
    Ok(
        (summary.len == prior.version.len && summary.hash.clone().finish() == prior.version.digest)
            .then(|| Tail::new(prior.version, remaining)),
    )
}

pub(super) fn read(
    path: &Path,
    previous: Option<&Contents>,
    remaining: &mut usize,
) -> io::Result<Option<Contents>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let current = identity(&file.metadata()?);
    let mut summary = Summary::default();
    let mut tail = prefix(&mut file, current, previous, &mut summary, *remaining)?;
    scan(&mut file, &mut summary, &mut tail)?;
    let version = Version {
        identity: current,
        len: summary.len,
        digest: summary.hash.finish(),
    };
    Ok(Some(Contents {
        version,
        has_newline: summary.has_newline,
        ended: summary.ended,
        appended: tail.map(|tail| tail.finish(remaining)),
    }))
}
