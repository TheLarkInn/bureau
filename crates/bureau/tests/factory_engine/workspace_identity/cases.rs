use serde_json::{Value, json};

const OPTIONAL: &[&str] = &[
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
    "host_type",
    "created_at",
    "updated_at",
    "user_named",
    "remote_steerable",
    "chronicle_sync_dismissed",
    "summary_count",
    "fork_count",
];

pub(super) fn invalid() -> Vec<(&'static str, Value)> {
    let mut cases: Vec<_> = OPTIONAL.iter().map(|&field| (field, Value::Null)).collect();
    cases.extend([
        ("host_type", json!("local")),
        ("summary_count", json!(-1)),
        ("summary_count", json!(1.5)),
        ("summary_count", json!(18_446_744_073_709_551_616.0)),
        ("fork_count", json!(18_446_744_073_709_551_616.0)),
        ("user_named", json!("true")),
        ("created_at", json!("2026-09-14T17:00:00+00:00")),
        ("created_at", json!("1900-02-29T17:00Z")),
        ("updated_at", json!("2026-04-31T17:00Z")),
        ("updated_at", json!("2026-09-14T24:00Z")),
        ("updated_at", json!("2026-09-14T17:00:00.Z")),
    ]);
    cases
}

pub(super) fn valid() -> Value {
    json!({
        "summary_count": 1.0, "fork_count": u64::MAX,
        "git_root": "original-git-root", "repository": "owner/repo", "branch": "main",
        "client_name": "bureau", "name": "", "summary": "Legacy label",
        "mc_task_id": "task", "mc_session_id": "session", "mc_last_event_id": "event",
        "mc_environment_id": "environment", "host_type": "ado",
        "user_named": true, "remote_steerable": false, "chronicle_sync_dismissed": false,
        "created_at": "2024-02-29T00:00Z", "updated_at": "2000-02-29T23:59:59.123456Z",
        "version": null, "unknown_metadata": {"nested": null}
    })
}
