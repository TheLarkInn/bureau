use std::path::Path;

use bureau::runlog::{Event, EventKind, RunState, RunStatus};
use serde::Serialize;

use super::{PAUSE_FILE, copilot_factory};
use crate::cli::out;

#[derive(Serialize)]
struct FactoryResume<'a> {
    session_id: &'a str,
    event_seq: Option<u64>,
    allowed: bool,
    reason: Option<String>,
}

fn continuation(directory: &Path, state: &RunState) -> anyhow::Result<()> {
    copilot_factory::check_state(state)?;
    anyhow::ensure!(
        matches!(state.status, RunStatus::Running),
        "run is already finished"
    );
    anyhow::ensure!(
        directory.join(PAUSE_FILE).try_exists()?,
        "run is not paused"
    );
    Ok(())
}

fn factory_resume<'a>(
    directory: &Path,
    state: &'a RunState,
    events: &[Event],
) -> Option<FactoryResume<'a>> {
    let record = copilot_factory::current(state)?;
    let reason = continuation(directory, state)
        .err()
        .map(|error| error.to_string());
    let event_seq = events
        .iter()
        .rfind(|event| event.kind == EventKind::CopilotFactory)
        .map(|event| event.seq);
    Some(FactoryResume {
        session_id: &record.intent.session_id,
        event_seq,
        allowed: reason.is_none(),
        reason,
    })
}

#[derive(Serialize)]
struct Projection<'a> {
    state: &'a RunState,
    local_factory_resume: Option<FactoryResume<'a>>,
}

pub(super) fn print(directory: &Path, state: &RunState, events: &[Event]) -> anyhow::Result<()> {
    let projection = Projection {
        state,
        local_factory_resume: factory_resume(directory, state, events),
    };
    out::line(format_args!("{}", serde_json::to_string(&projection)?));
    Ok(())
}
