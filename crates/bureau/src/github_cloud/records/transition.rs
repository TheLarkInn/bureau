use serde_json::Value;

use super::{Dispatch, Record, State, validate};

fn prepare(state: &mut State, event: &str) -> Result<(), &'static str> {
    if state.dispatch != Dispatch::NotSubmitted || state.task_id.is_some() {
        return Err("dispatch can only be prepared on a new, unselected receipt");
    }
    if !matches!(event, "manual" | "interval") {
        return Err("unsupported cloud dispatch event");
    }
    state.dispatch = Dispatch::Prepared;
    state.event = Some(event.to_owned());
    Ok(())
}

fn complete(
    state: &mut State,
    dispatch: Dispatch,
    message: Option<&str>,
) -> Result<(), &'static str> {
    if state.dispatch != Dispatch::Prepared {
        return Err("dispatch completion requires a prepared submission");
    }
    state.dispatch = dispatch;
    state.dispatch_message = message.map(str::to_owned);
    Ok(())
}

fn select_task(state: &mut State, task_id: &str) -> Result<(), &'static str> {
    validate::opaque(task_id)?;
    if state
        .task_id
        .as_deref()
        .is_some_and(|selected| selected != task_id)
    {
        return Err("the selected cloud task cannot be changed");
    }
    state.task_id = Some(task_id.to_owned());
    state.task_correlation = Some("operator_selected_unproven");
    Ok(())
}

fn observe(
    state: &mut State,
    task: &Value,
    events: Option<&[Value]>,
    reported_total: Option<u32>,
    at_ms: u64,
) -> Result<(), &'static str> {
    if events.is_none() && reported_total.is_some() {
        return Err("an event total requires a new event observation");
    }
    validate::observation(state, task, events)?;
    state.task = Some(task.clone());
    state.observed_at_ms = Some(at_ms);
    if let Some(events) = events {
        state.events = events.to_vec();
        state.events_reported_total = reported_total;
        state.events_observed_at_ms = Some(at_ms);
    }
    Ok(())
}

pub(super) fn apply(state: &mut State, record: &Record, at_ms: u64) -> Result<(), &'static str> {
    match record {
        Record::Prepared { event } => prepare(state, event),
        Record::Accepted => complete(state, Dispatch::Accepted, None),
        Record::Rejected { message } => complete(state, Dispatch::Rejected, Some(message)),
        Record::Uncertain { message } => complete(state, Dispatch::Uncertain, Some(message)),
        Record::TaskSelected { task_id } => select_task(state, task_id),
        Record::Observed {
            task,
            events,
            reported_total,
        } => observe(state, task, events.as_deref(), *reported_total, at_ms),
    }
}
