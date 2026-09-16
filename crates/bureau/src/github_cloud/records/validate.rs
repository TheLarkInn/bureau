use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{Scope, Start, State};
use crate::runlog::ConfigSource;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    remote: String,
    reference: String,
    commit: String,
}

pub(super) fn config_source<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<ConfigSource, D::Error> {
    let source = Source::deserialize(deserializer)?;
    Ok(ConfigSource {
        remote: source.remote,
        reference: source.reference,
        commit: source.commit,
    })
}

pub(super) fn opaque(value: &str) -> Result<(), &'static str> {
    let valid = !value.is_empty()
        && value.len() <= 1024
        && value.trim() == value
        && !value.chars().any(char::is_control);
    if valid {
        Ok(())
    } else {
        Err("missing or malformed cloud identity")
    }
}

fn repo_owner(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn repo_name(value: &str) -> bool {
    !matches!(value, "" | "." | "..")
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn repository(value: &str) -> Result<(), &'static str> {
    let (owner, name) = value
        .split_once('/')
        .ok_or("repository must be a canonical owner/name")?;
    if repo_owner(owner) && repo_name(name) {
        Ok(())
    } else {
        Err("repository must be a canonical owner/name")
    }
}

fn scope(scope: &Scope) -> Result<(), &'static str> {
    repository(&scope.repo)?;
    if scope.principal_id == 0 {
        return Err("cloud scope has no verified principal");
    }
    for value in [
        &scope.registry_name,
        &scope.credential_reference,
        &scope.principal_login,
        &scope.config_source.remote,
        &scope.config_source.reference,
        &scope.config_source.commit,
    ] {
        opaque(value)?;
    }
    Ok(())
}

pub(super) fn start(start: &Start) -> Result<(), &'static str> {
    scope(&start.scope)?;
    opaque(&start.request_id)?;
    opaque(&start.automation_id)?;
    if !start.definition.is_object() {
        return Err("cloud definition must be an object");
    }
    Ok(())
}

fn exact(value: &Value, field: &str, expected: &str) -> Result<(), &'static str> {
    if value.get(field).and_then(Value::as_str) == Some(expected) {
        Ok(())
    } else {
        Err("cloud observation identity does not match the selected task and automation")
    }
}

fn sessions(task: &Value, task_id: &str) -> Result<(), &'static str> {
    match task.get("sessions") {
        None => Ok(()),
        Some(Value::Array(sessions)) => sessions
            .iter()
            .try_for_each(|session| exact(session, "task_id", task_id)),
        Some(_) => Err("task sessions must be an array when supplied"),
    }
}

fn event_objects(events: Option<&[Value]>) -> Result<(), &'static str> {
    if events.is_some_and(|events| events.iter().any(|event| !event.is_object())) {
        return Err("cloud events must be objects");
    }
    Ok(())
}

pub(super) fn observation(
    state: &State,
    task: &Value,
    events: Option<&[Value]>,
) -> Result<(), &'static str> {
    let task_id = state
        .task_id
        .as_deref()
        .ok_or("select an exact task before recording observations")?;
    exact(task, "id", task_id)?;
    exact(task, "automation_id", &state.start.automation_id)?;
    sessions(task, task_id)?;
    event_objects(events)
}
