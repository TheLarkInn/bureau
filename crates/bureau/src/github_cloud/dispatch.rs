use crate::forge::github::cloud::{self, AutomationId, Definition};
use crate::state::LeaseOwner;

use super::control::{require_owner, supervised};
use super::{Control, Error, Log, Record, State};

fn completion(sent: Result<(), cloud::Error>) -> Record {
    match sent {
        Ok(()) => Record::Accepted,
        Err(error) if error.is_definite_rejection() => Record::Rejected {
            message: error.to_string(),
        },
        Err(error) => Record::Uncertain {
            message: error.to_string(),
        },
    }
}

fn record_completion(log: &mut Log, owner: &LeaseOwner, record: &Record) -> Result<(), Error> {
    log.append(owner, record)
        .map_err(|error| Error::UncertainRecord {
            request_id: log.state().start.request_id.clone(),
            message: error.to_string(),
        })
}

async fn send_once(
    control: &Control<'_>,
    owner: &LeaseOwner,
    definition: &Definition,
    mut log: Log,
) -> Result<State, Error> {
    let event = definition.dispatch_event()?.as_str().to_owned();
    log.append(owner, &Record::Prepared { event })?;
    require_owner(owner)?;
    let sent = control
        .client
        .dispatch(control.selection.repo(), definition)
        .await;
    record_completion(&mut log, owner, &completion(sent))?;
    Ok(log.state().clone())
}

async fn dispatch_owned(
    control: &Control<'_>,
    key: &str,
    automation: &AutomationId,
    owner: &LeaseOwner,
) -> Result<State, Error> {
    if let Some(state) = control.existing(key, automation)? {
        return Ok(state);
    }
    let definition = control
        .client
        .definition(control.selection.repo(), automation)
        .await?;
    definition.dispatch_event()?;
    require_owner(owner)?;
    let log = control.create(key, &definition, owner)?;
    send_once(control, owner, &definition, log).await
}

/// Makes at most one submission for a stable local key.
///
/// # Errors
/// Rejects invalid grants, conflicting ownership/identity, and failed durable writes.
/// Existing prepared/accepted/uncertain receipts are never redispatched.
pub async fn dispatch(
    control: &Control<'_>,
    key: &str,
    automation: &AutomationId,
) -> Result<State, Error> {
    control.selection.require_dispatch()?;
    let owner = control.owner(key)?;
    let future = dispatch_owned(control, key, automation, &owner);
    supervised(control, key, &owner, future).await
}
