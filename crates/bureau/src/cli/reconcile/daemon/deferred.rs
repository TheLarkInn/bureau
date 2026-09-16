use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use bureau::runlog::copilot_factory::{Record, Records};
use bureau::runlog::{RunSnapshot, RunState};
use bureau::state::LeaseOwner;

use crate::cli::factory_credentials::ModelSourceError;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn directory(root: &Path, run_id: &str) -> io::Result<PathBuf> {
    if Path::new(run_id).file_name() != Some(std::ffi::OsStr::new(run_id)) {
        return Err(invalid(
            "preserved factory run identity is not a directory name",
        ));
    }
    let directory = root.join(run_id);
    if !std::fs::symlink_metadata(&directory)?.is_dir() {
        return Err(invalid(
            "preserved factory run directory is missing or replaced",
        ));
    }
    Ok(directory)
}

fn snapshot_identity(state: &RunState, snapshot: &RunSnapshot) -> io::Result<()> {
    if state.run_id != snapshot.run_id
        || state.assignment != snapshot.assignment.name
        || state.snapshot.as_ref() != Some(snapshot)
    {
        return Err(invalid("preserved factory snapshot identity changed"));
    }
    Ok(())
}

fn saved_reference(snapshot: &RunSnapshot, reference: &str) -> io::Result<()> {
    if !snapshot
        .pipeline
        .factory_credential_refs()
        .any(|saved| saved == reference)
    {
        return Err(invalid(
            "unavailable model credential is absent from its saved pipeline",
        ));
    }
    Ok(())
}

fn known_records(state: &RunState) -> io::Result<&Records> {
    if let Some(error) = &state.copilot_factory_error {
        return Err(invalid(format!(
            "preserved factory log is corrupt: {error}"
        )));
    }
    if state.copilot_factories.0.is_empty() {
        return Err(invalid(
            "credential deferral requires a known preserved factory identity",
        ));
    }
    Ok(&state.copilot_factories)
}

fn workspace(directory: &Path, record: &Record) -> io::Result<()> {
    let actual = std::fs::canonicalize(directory.join("wt"))?;
    let metadata = std::fs::metadata(&actual)?;
    let expected = &record.intent.workspace;
    if actual != expected.directory
        || metadata.dev() != expected.device
        || metadata.ino() != expected.inode
    {
        return Err(invalid("preserved factory workspace identity changed"));
    }
    Ok(())
}

fn replay(directory: &Path) -> io::Result<RunState> {
    let events = bureau::runlog::read_events_tolerant(directory)?;
    bureau::runlog::replay(events)
        .ok_or_else(|| invalid("preserved factory log has no run_started event"))
}

fn verify_record(directory: &Path, snapshot: &RunSnapshot, record: &Record) -> io::Result<()> {
    let step = snapshot
        .pipeline
        .steps
        .iter()
        .find(|step| step.name == record.intent.step)
        .ok_or_else(|| invalid("preserved factory step is absent from its saved pipeline"))?;
    if step.copilot_factory.as_ref() != Some(&record.intent.factory) {
        return Err(invalid("preserved factory differs from its saved pipeline"));
    }
    workspace(directory, record)
}

fn verify(root: &Path, snapshot: &RunSnapshot, reference: &str) -> io::Result<()> {
    let directory = directory(root, &snapshot.run_id)?;
    let state = replay(&directory)?;
    snapshot_identity(&state, snapshot)?;
    saved_reference(snapshot, reference)?;
    for record in known_records(&state)?.0.values() {
        verify_record(&directory, snapshot, record)?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error(
    "factory run `{run_id}` for assignment `{assignment}` deferred: model credential \
     `{reference}` is unavailable: {message}; saved state remains preserved"
)]
pub(super) struct Deferred {
    run_id: String,
    assignment: String,
    reference: String,
    message: String,
}

impl Deferred {
    pub(super) fn from_model_source(snapshot: &RunSnapshot, error: &anyhow::Error) -> Option<Self> {
        let reference = error
            .downcast_ref::<ModelSourceError>()?
            .unavailable_reference()?;
        Some(Self {
            run_id: snapshot.run_id.clone(),
            assignment: snapshot.assignment.name.clone(),
            reference: reference.to_owned(),
            message: error.to_string(),
        })
    }

    pub(super) fn preserve(
        &self,
        root: &Path,
        snapshot: &RunSnapshot,
        owner: &LeaseOwner,
    ) -> anyhow::Result<()> {
        let verified = owner.with_ownership(|| verify(root, snapshot, &self.reference));
        let released = owner.release();
        verified?;
        released?;
        Ok(())
    }
}
