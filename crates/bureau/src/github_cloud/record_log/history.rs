use std::path::Path;

use super::{Error, envelope};
use crate::github_cloud::records::State;
use crate::runlog::{EVENTS_FILE, Event, EventKind};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct History {
    pub(super) state: State,
    pub(super) next_seq: u64,
}

fn line_event(line: &str, last: bool) -> Result<Option<Event>, Error> {
    match serde_json::from_str::<Event>(line) {
        Err(error) if last && error.is_eof() => Ok(None),
        Err(error) => Err(error.into()),
        Ok(event) => Ok(Some(event)),
    }
}

fn read_events(dir: &Path) -> Result<Vec<Event>, Error> {
    let text = std::fs::read_to_string(dir.join(EVENTS_FILE))?;
    let lines: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let mut events = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(event) = line_event(line, index + 1 == lines.len())? {
            events.push(event);
        }
    }
    Ok(events)
}

fn apply(state: Option<State>, event: &Event, expected: u64) -> Result<State, Error> {
    if event.seq != expected {
        return Err(Error::InvalidHistory(
            "cloud sequences must be contiguous from zero",
        ));
    }
    if event.kind != EventKind::GitHubCloud {
        return Err(Error::InvalidHistory("non-cloud event in a cloud receipt"));
    }
    envelope::apply(state, &event.data, event.at_ms)
}

pub(super) fn read(dir: &Path, key: &str) -> Result<History, Error> {
    let mut state = None;
    let mut next_seq = 0;
    for event in read_events(dir)? {
        state = Some(apply(state, &event, next_seq)?);
        next_seq += 1;
    }
    let state = state.ok_or(Error::InvalidHistory("cloud receipt has no created record"))?;
    if state.start.request_id != key {
        return Err(Error::InvalidHistory(
            "stored request key does not match its directory",
        ));
    }
    Ok(History { state, next_seq })
}
