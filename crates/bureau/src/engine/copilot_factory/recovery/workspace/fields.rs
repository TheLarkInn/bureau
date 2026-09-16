//! Validate native known fields without migrating labels or accepting null as absence.
//! Bureau rejects summary-count overflow rather than using native cwd-recovery saturation.

use serde_yaml_ng::{Mapping, Value};

use super::timestamp;

const STRINGS: &[&str] = &[
    "git_root",
    "repository",
    "branch",
    "client_name",
    "name",
    "mc_task_id",
    "mc_session_id",
    "mc_last_event_id",
    "mc_environment_id",
    "summary",
];

const BOOLEANS: &[&str] = &["user_named", "remote_steerable", "chronicle_sync_dismissed"];

fn count(value: &Value) -> bool {
    let Value::Number(number) = value else {
        return false;
    };
    number.as_u64().is_some()
        || number.as_f64().is_some_and(|number| {
            (0.0..18_446_744_073_709_551_616.0).contains(&number)
                && number.fract().classify() == std::num::FpCategory::Zero
        })
}

fn host_type(value: &Value) -> bool {
    matches!(value.as_str(), Some("github" | "ado"))
}

const fn string(value: &Value) -> bool {
    matches!(value, Value::String(_))
}

const fn boolean(value: &Value) -> bool {
    matches!(value, Value::Bool(_))
}

fn valid(name: &str, value: &Value) -> bool {
    match name {
        "summary_count" | "fork_count" => count(value),
        "host_type" => host_type(value),
        "created_at" | "updated_at" => value.as_str().is_some_and(timestamp::valid),
        name if STRINGS.contains(&name) => string(value),
        name if BOOLEANS.contains(&name) => boolean(value),
        _ => true,
    }
}

pub(super) fn verify(mapping: &Mapping) -> Result<(), String> {
    for (name, value) in mapping {
        if name.as_str().is_some_and(|name| !valid(name, value)) {
            return Err(format!(
                "known workspace field {name:?} has an invalid value"
            ));
        }
    }
    Ok(())
}
