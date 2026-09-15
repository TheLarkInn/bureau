//! Ownership-fenced, scrubbed cloud receipts without a derived state cache.

mod envelope;
mod fence;
mod history;
mod write;

#[cfg(test)]
mod tests;

use std::path::Path;

use serde_json::Value;

use super::records::{Record, Start, State};
use crate::process::Secret;
use crate::runlog::RunLog;
use crate::state::LeaseOwner;

/// Cloud receipt, history, or ownership failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Filesystem creation, reading, writing, or synchronization failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A record could not be serialized or strictly decoded.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The receipt cannot be replayed or the proposed change is invalid.
    #[error("invalid cloud history: {0}")]
    InvalidHistory(&'static str),
    /// A local key is not a safe, bounded path component.
    #[error("cloud request key must contain 1-128 ASCII letters, digits, hyphens, or underscores")]
    InvalidKey,
    /// This owner does not hold the live lease for this exact receipt.
    #[error("cloud receipt requires its own live, matching lease")]
    Ownership,
    /// The durable store could not prove ownership.
    #[error("cloud lease ownership could not be verified")]
    OwnershipCheck(#[source] crate::state::Error),
}

/// Disjoint lease assignment for explicit cloud controls.
pub const LEASE_ASSIGNMENT: &str = "github-cloud-controls";

/// The external lease key, distinct from an automation or task identity.
#[must_use]
pub fn lease_key(repo: &str, request_id: &str) -> String {
    format!("{repo}/{request_id}")
}

/// Validates a local request key, not an opaque remote identifier.
///
/// # Errors
/// Rejects empty, oversized, non-ASCII, control, traversal, or separator input.
pub fn validate_key(key: &str) -> Result<(), Error> {
    let valid = !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidKey)
    }
}

fn ownership(owner: &LeaseOwner, start: &Start) -> Result<(), Error> {
    if owner.assignment() != LEASE_ASSIGNMENT
        || owner.external_id() != lease_key(&start.scope.repo, &start.request_id)
    {
        return Err(Error::Ownership);
    }
    if !owner.owns().map_err(Error::OwnershipCheck)? {
        return Err(Error::Ownership);
    }
    Ok(())
}

fn unchanged_start(expected: &Start, actual: &Start) -> Result<(), Error> {
    if expected.scope != actual.scope
        || expected.request_id != actual.request_id
        || expected.automation_id != actual.automation_id
    {
        return Err(Error::InvalidHistory("immutable receipt identity changed"));
    }
    Ok(())
}

fn preview(state: Option<&State>, data: &Value, secrets: &[Secret]) -> Result<State, Error> {
    let original = envelope::apply(state.cloned(), data, 0)?;
    let scrubbed = envelope::apply(state.cloned(), &envelope::scrubbed(data, secrets)?, 0)?;
    unchanged_start(&original.start, &scrubbed.start)?;
    if original.task_id != scrubbed.task_id {
        return Err(Error::InvalidHistory(
            "scrubbing would change the selected task identity",
        ));
    }
    Ok(scrubbed)
}

/// Replays a receipt read-only, without repairing a torn final line.
///
/// # Errors
/// Rejects unsafe keys, corrupt history, invalid transitions, and unknown versions.
pub fn read_state(root: &Path, key: &str) -> Result<State, Error> {
    validate_key(key)?;
    Ok(history::read(&root.join(key), key)?.state)
}

/// An exclusively owned durable receipt. A failed write requires reopening.
pub struct Log {
    writer: Option<RunLog>,
    state: State,
    next_seq: u64,
    secrets: Vec<Secret>,
}

impl Log {
    /// Creates a new, durable events-only receipt under the matching live lease.
    ///
    /// # Errors
    /// Propagates identity, ownership, serialization, creation, and sync failures.
    pub fn create(
        root: &Path,
        start: Start,
        secrets: &[Secret],
        owner: &LeaseOwner,
    ) -> Result<Self, Error> {
        validate_key(&start.request_id)?;
        let data = envelope::created(start)?;
        let state = preview(None, &data, secrets)?;
        ownership(owner, &state.start)?;
        let key = &state.start.request_id;
        let (writer, persisted) = write::create(owner, root, key, secrets, data)?;
        if state != persisted.state {
            return Err(Error::InvalidHistory("new receipt changed during readback"));
        }
        Ok(Self {
            writer: Some(writer),
            state: persisted.state,
            next_seq: persisted.next_seq,
            secrets: secrets.to_vec(),
        })
    }

    /// Opens a receipt after strict read-only validation and ownership binding.
    /// Only then may the underlying log repair its torn tail.
    ///
    /// # Errors
    /// Propagates replay, ownership, filesystem, and repair failures.
    pub fn open(
        root: &Path,
        key: &str,
        secrets: &[Secret],
        owner: &LeaseOwner,
    ) -> Result<Self, Error> {
        validate_key(key)?;
        let previous = history::read(&root.join(key), key)?;
        ownership(owner, &previous.state.start)?;
        let writer = write::resume(owner, &root.join(key), secrets)?;
        let current = history::read(writer.dir(), key)?;
        if previous != current {
            return Err(Error::InvalidHistory("receipt changed while opening"));
        }
        Ok(Self {
            writer: Some(writer),
            state: current.state,
            next_seq: current.next_seq,
            secrets: secrets.to_vec(),
        })
    }

    /// The last successfully durably recorded, scrubbed state.
    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    fn append_with(
        &mut self,
        owner: &LeaseOwner,
        record: &Record,
        append: impl FnOnce(&mut RunLog, Value) -> std::io::Result<u64>,
    ) -> Result<(), Error> {
        ownership(owner, &self.state.start)?;
        let data = envelope::record(record)?;
        let next = preview(Some(&self.state), &data, &self.secrets)?;
        unchanged_start(&self.state.start, &next.start)?;
        let writer = self
            .writer
            .take()
            .ok_or(Error::InvalidHistory("reopen failed writer"))?;
        let key = &self.state.start.request_id;
        let (writer, persisted) = write::append(owner, writer, key, self.next_seq, data, append)?;
        unchanged_start(&next.start, &persisted.state.start)?;
        self.state = persisted.state;
        self.next_seq = persisted.next_seq;
        self.writer = Some(writer);
        Ok(())
    }

    /// Validates a prospective transition before its scrubbed, fsync'd append.
    /// The live lease generation is fenced through every filesystem mutation.
    ///
    /// # Errors
    /// Propagates every failure; a write/readback failure disables further appends
    /// on this handle and never changes its in-memory state.
    pub fn append(&mut self, owner: &LeaseOwner, record: &Record) -> Result<(), Error> {
        self.append_with(owner, record, write::event)
    }

    /// Closes the log, exposing final flush and synchronization failures.
    ///
    /// # Errors
    /// Propagates writer failures, including a previously failed append.
    pub fn close(mut self) -> Result<(), Error> {
        self.writer
            .take()
            .ok_or(Error::InvalidHistory("reopen failed writer"))?
            .close()?;
        Ok(())
    }
}
