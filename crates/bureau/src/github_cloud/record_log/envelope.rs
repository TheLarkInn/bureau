use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Error;
use crate::github_cloud::records::{self, Record, Start, State};
use crate::process::{Secret, scrub_json};

const SCHEMA: &str = "github_cloud_v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: String,
    record: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CreatedKind {
    Created,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Created {
    kind: CreatedKind,
    start: Start,
}

fn secret_key(key: &str, secrets: &[Secret]) -> Result<(), Error> {
    let mut value = Value::String(key.to_owned());
    scrub_json(&mut value, secrets);
    if value.as_str() != Some(key) {
        return Err(Error::InvalidHistory(
            "a secret occurs in a JSON object key",
        ));
    }
    Ok(())
}

fn secret_keys(data: &Value, secrets: &[Secret]) -> Result<(), Error> {
    match data {
        Value::Object(values) => values.iter().try_for_each(|(key, value)| {
            secret_key(key, secrets)?;
            secret_keys(value, secrets)
        }),
        Value::Array(values) => values
            .iter()
            .try_for_each(|value| secret_keys(value, secrets)),
        _ => Ok(()),
    }
}

pub(super) fn scrubbed(data: &Value, secrets: &[Secret]) -> Result<Value, Error> {
    secret_keys(data, secrets)?;
    let mut value = data.clone();
    scrub_json(&mut value, secrets);
    Ok(value)
}

fn wrap(record: Value) -> Result<Value, Error> {
    Ok(serde_json::to_value(Envelope {
        schema: SCHEMA.to_owned(),
        record,
    })?)
}

pub(super) fn created(start: Start) -> Result<Value, Error> {
    wrap(serde_json::to_value(Created {
        kind: CreatedKind::Created,
        start,
    })?)
}

pub(super) fn record(record: &Record) -> Result<Value, Error> {
    wrap(serde_json::to_value(record)?)
}

fn initial(state: Option<&State>, record: Value) -> Result<State, Error> {
    if state.is_some() {
        return Err(Error::InvalidHistory(
            "created must be the first and only creation record",
        ));
    }
    let created: Created = serde_json::from_value(record)?;
    records::created(created.start).map_err(Error::InvalidHistory)
}

fn strict_record(value: Value) -> Result<Record, Error> {
    let fields = value
        .as_object()
        .ok_or(Error::InvalidHistory("cloud record must be an object"))?;
    // Serde's internally tagged unit variants otherwise discard extra fields.
    if fields.get("kind").and_then(Value::as_str) == Some("accepted") && fields.len() != 1 {
        return Err(Error::InvalidHistory(
            "accepted record has unexpected fields",
        ));
    }
    Ok(serde_json::from_value(value)?)
}

pub(super) fn apply(state: Option<State>, data: &Value, at_ms: u64) -> Result<State, Error> {
    let envelope: Envelope = serde_json::from_value(data.clone())?;
    if envelope.schema != SCHEMA {
        return Err(Error::InvalidHistory("unsupported cloud record schema"));
    }
    if envelope.record.get("kind").and_then(Value::as_str) == Some("created") {
        return initial(state.as_ref(), envelope.record);
    }
    let mut state = state.ok_or(Error::InvalidHistory(
        "cloud history must start with created",
    ))?;
    let record = strict_record(envelope.record)?;
    state.apply(&record, at_ms).map_err(Error::InvalidHistory)?;
    Ok(state)
}
