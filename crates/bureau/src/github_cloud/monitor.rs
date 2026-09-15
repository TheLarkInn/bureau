use crate::forge::github::cloud::{AutomationId, Client, Task, TaskId};
use crate::state::LeaseOwner;

use super::control::{require_owner, supervised};
use super::{Control, Error, Log, Record, State};

async fn observed(
    client: &Client,
    automation: &AutomationId,
    task: &TaskId,
    events: bool,
) -> Result<Record, Error> {
    let first = client.task(automation, task).await?;
    if !events {
        return Ok(Record::Observed {
            task: serde_json::to_value(first)?,
            events: None,
            reported_total: None,
        });
    }
    let events = client.events(task).await?;
    let latest = client.task(automation, task).await?;
    Ok(Record::Observed {
        task: serde_json::to_value(latest)?,
        events: Some(events.events),
        reported_total: events.reported_total,
    })
}

fn check_selection(state: Option<&State>, task: &Task) -> Result<(), Error> {
    let selected = state.and_then(|state| state.task_id.as_deref());
    if selected.is_some_and(|selected| selected != task.id.as_str()) {
        return Err(Error::Selection(
            "a recorded task selection cannot be replaced".to_owned(),
        ));
    }
    if selected.is_none() && task.archived_at.is_some() {
        return Err(Error::Selection(
            "an archived task cannot be newly attached".to_owned(),
        ));
    }
    Ok(())
}

fn check_existing_task(state: Option<&State>, task: &TaskId) -> Result<(), Error> {
    let selected = state.and_then(|state| state.task_id.as_deref());
    if selected.is_some_and(|selected| selected != task.as_str()) {
        return Err(Error::Selection(
            "a recorded task selection cannot be replaced".to_owned(),
        ));
    }
    Ok(())
}

fn append_task(log: &mut Log, owner: &LeaseOwner, task: Task) -> Result<State, Error> {
    log.append(
        owner,
        &Record::TaskSelected {
            task_id: task.id.as_str().to_owned(),
        },
    )?;
    let record = Record::Observed {
        task: serde_json::to_value(task)?,
        events: None,
        reported_total: None,
    };
    log.append(owner, &record)?;
    Ok(log.state().clone())
}

async fn track_owned(
    control: &Control<'_>,
    key: &str,
    automation: &AutomationId,
    task_id: &TaskId,
    owner: &LeaseOwner,
) -> Result<State, Error> {
    let existing = control.existing(key, automation)?;
    check_existing_task(existing.as_ref(), task_id)?;
    let definition = control
        .client
        .definition(control.selection.repo(), automation)
        .await?;
    let task = control.client.task(automation, task_id).await?;
    check_selection(existing.as_ref(), &task)?;
    require_owner(owner)?;
    let mut log = if existing.is_some() {
        control.open(key, owner)?
    } else {
        control.create(key, &definition, owner)?
    };
    append_task(&mut log, owner, task)
}

async fn refresh_owned(
    control: &Control<'_>,
    key: &str,
    owner: &LeaseOwner,
    events: bool,
) -> Result<State, Error> {
    let mut log = control.open(key, owner)?;
    let automation = AutomationId::try_from(log.state().start.automation_id.clone())?;
    control.selection.check_binding(log.state(), &automation)?;
    let task = log.state().task_id.clone().ok_or_else(|| {
        Error::Selection(
            "receipt has no operator-selected cloud task; select an exact task ID first".to_owned(),
        )
    })?;
    let task = TaskId::try_from(task)?;
    control
        .client
        .definition(control.selection.repo(), &automation)
        .await?;
    let record = observed(control.client, &automation, &task, events).await?;
    log.append(owner, &record)?;
    Ok(log.state().clone())
}

/// Records an operator-selected task, never a claimed dispatch-to-task correlation.
///
/// # Errors
/// Rejects unverifiable/archived selections, conflicting records, and ownership failures.
pub async fn track(
    control: &Control<'_>,
    key: &str,
    automation: &AutomationId,
    task: &TaskId,
) -> Result<State, Error> {
    let owner = control.owner(key)?;
    let future = track_owned(control, key, automation, task, &owner);
    supervised(control, key, &owner, future).await
}

/// Refreshes exact selected-task observations; this does not resume remote execution.
///
/// # Errors
/// Rejects identity drift, unreadable receipts, missing selection, or incomplete observations.
pub async fn refresh(control: &Control<'_>, key: &str, events: bool) -> Result<State, Error> {
    let owner = control.owner(key)?;
    let future = refresh_owned(control, key, &owner, events);
    supervised(control, key, &owner, future).await
}
