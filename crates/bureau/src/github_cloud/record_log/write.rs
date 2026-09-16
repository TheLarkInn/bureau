use std::path::Path;

use serde_json::Value;

use super::{Error, fence, history};
use crate::process::Secret;
use crate::runlog::{EventKind, RunLog};
use crate::state::LeaseOwner;

pub(super) fn event(writer: &mut RunLog, data: Value) -> std::io::Result<u64> {
    writer.append(EventKind::GitHubCloud, data)
}

fn readback(
    writer: RunLog,
    key: &str,
    expected: u64,
    sequence: u64,
) -> Result<(RunLog, history::History), Error> {
    if sequence != expected {
        return Err(Error::InvalidHistory("cloud writer sequence changed"));
    }
    let persisted = history::read(writer.dir(), key)?;
    let next = expected
        .checked_add(1)
        .ok_or(Error::InvalidHistory("cloud sequence overflow"))?;
    if persisted.next_seq != next {
        return Err(Error::InvalidHistory(
            "cloud append could not be read back exactly",
        ));
    }
    Ok((writer, persisted))
}

fn appended(
    mut writer: RunLog,
    data: Value,
    append: impl FnOnce(&mut RunLog, Value) -> std::io::Result<u64>,
) -> std::io::Result<(RunLog, u64)> {
    // On failure, dispose of the buffered writer before the fence is released.
    let sequence = append(&mut writer, data)?;
    Ok((writer, sequence))
}

pub(super) fn create(
    owner: &LeaseOwner,
    root: &Path,
    key: &str,
    secrets: &[Secret],
    data: Value,
) -> Result<(RunLog, history::History), Error> {
    let (writer, sequence) = fence::run(owner, || {
        appended(RunLog::create_events(root, key, secrets)?, data, event)
    })?;
    readback(writer, key, 0, sequence)
}

pub(super) fn resume(owner: &LeaseOwner, dir: &Path, secrets: &[Secret]) -> Result<RunLog, Error> {
    fence::run(owner, || RunLog::resume(dir, secrets))
}

pub(super) fn append(
    owner: &LeaseOwner,
    writer: RunLog,
    key: &str,
    expected: u64,
    data: Value,
    append: impl FnOnce(&mut RunLog, Value) -> std::io::Result<u64>,
) -> Result<(RunLog, history::History), Error> {
    let (writer, sequence) = fence::run(owner, move || appended(writer, data, append))?;
    readback(writer, key, expected, sequence)
}
