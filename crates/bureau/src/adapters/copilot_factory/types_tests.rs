use serde_json::{Value, json};

use super::{
    FactoryResumeResult, FactoryRunConsumed, FactoryRunResult, FactoryRunStatus, FactoryRunSummary,
};

fn envelope(status: &str) -> Value {
    json!({"runId": "run-1", "status": status})
}

fn summary() -> Value {
    json!({
        "runId": "run-1", "factoryName": "verify", "status": "running",
        "consumed": {"activeMs": 12, "subagents": 2, "nanoAiu": 9_007_199_254_740_993_u64},
    })
}

fn decode(value: Value) -> FactoryRunResult {
    serde_json::from_value(value).expect("run envelope")
}

#[test]
fn settled_and_resume_candidate_states_are_explicit() {
    for (name, settled, candidate) in [
        ("pending", false, false),
        ("running", false, false),
        ("completed", true, false),
        ("error", true, true),
        ("cancelled", true, false),
        ("halted", true, true),
        ("paused", true, true),
    ] {
        let status: FactoryRunStatus = serde_json::from_value(json!(name)).expect("status");
        assert_eq!(
            (status.is_settled(), status.is_resume_candidate()),
            (settled, candidate)
        );
    }
}

#[test]
fn missing_invalid_identity_or_unknown_status_fails() {
    for value in [
        json!({}),
        json!({"runId": "run-1"}),
        json!({"status": "running"}),
        json!({"runId": "", "status": "running"}),
        json!({"runId": " \n", "status": "running"}),
        json!({"runId": 1, "status": "running"}),
        json!({"runId": "run-1", "status": "unknown"}),
        json!({"runId": "run-1", "status": null}),
    ] {
        assert!(serde_json::from_value::<FactoryRunResult>(value).is_err());
    }
}

#[test]
fn arbitrary_result_including_explicit_null_is_preserved() {
    for result in [
        Value::Null,
        json!(true),
        json!(12),
        json!("text"),
        json!([1]),
        json!({"x": 1}),
    ] {
        let mut value = envelope("completed");
        value["result"] = result.clone();
        let decoded = decode(value);
        assert_eq!(decoded.result, Some(result));
    }
}

#[test]
fn absent_result_does_not_become_json_null() {
    let value = serde_json::to_value(decode(envelope("completed"))).expect("encode");
    let mut explicit_null = envelope("completed");
    explicit_null["result"] = Value::Null;
    let explicit_null = serde_json::to_value(decode(explicit_null)).expect("encode null");
    assert_eq!(
        (value.get("result"), explicit_null.get("result")),
        (None, Some(&Value::Null))
    );
}

#[test]
fn structured_failure_and_snapshot_are_preserved() {
    let failure =
        json!({"type": "factory_accounting_incomplete", "runId": "run-1", "drainedNanoAiu": 15});
    let snapshot = json!({"journal": [{"name": "verify", "result": {"ok": true}}]});
    let mut value = envelope("error");
    value["error"] = json!("accounting did not drain");
    value["reason"] = json!("interrupted");
    value["failure"] = failure;
    value["snapshot"] = snapshot;
    let decoded = serde_json::to_value(decode(value.clone())).expect("encode");
    assert_eq!(decoded, value);
}

#[test]
fn newer_attempt_and_pause_fields_are_retained() {
    let mut value = envelope("paused");
    value["attempt"] = json!(2);
    value["pauseInfo"] = json!({"type": "checkpoint", "key": "review", "futureField": 1});
    let decoded = decode(value);
    assert_eq!(
        (
            decoded.attempt.map(std::num::NonZeroU64::get),
            decoded.pause_info
        ),
        (
            Some(2),
            Some(json!({"type": "checkpoint", "key": "review", "futureField": 1}))
        )
    );
}

#[test]
fn invalid_attempts_fail() {
    for attempt in [json!(0), json!(-1), json!(1.5), json!("2"), Value::Null] {
        let mut value = envelope("running");
        value["attempt"] = attempt;
        assert!(serde_json::from_value::<FactoryRunResult>(value).is_err());
    }
}

#[test]
fn unknown_states_are_not_treated_as_settled() {
    for status in ["unknown", "interrupted", "success", "Running"] {
        assert!(serde_json::from_value::<FactoryRunStatus>(json!(status)).is_err());
    }
}

#[test]
fn older_envelopes_need_no_attempt_or_accounting() {
    let decoded = decode(envelope("running"));
    assert_eq!(
        (decoded.attempt, decoded.result, decoded.pause_info),
        (None, None, None)
    );
}

#[test]
fn accounting_is_exact_and_required_on_summaries() {
    let decoded: FactoryRunSummary = serde_json::from_value(summary()).expect("summary");
    assert_eq!(
        (decoded.consumed, decoded.can_resume),
        (
            FactoryRunConsumed {
                active_ms: 12,
                subagents: 2,
                nano_aiu: 9_007_199_254_740_993
            },
            None
        )
    );
}

#[test]
fn missing_summary_identity_status_and_consumption_fail() {
    for field in ["runId", "factoryName", "status", "consumed"] {
        let mut value = summary();
        value.as_object_mut().expect("object").remove(field);
        assert!(
            serde_json::from_value::<FactoryRunSummary>(value).is_err(),
            "{field}"
        );
    }
}

#[test]
fn missing_accounting_counters_fail() {
    for field in ["activeMs", "subagents", "nanoAiu"] {
        let mut value = summary();
        value["consumed"]
            .as_object_mut()
            .expect("object")
            .remove(field);
        assert!(
            serde_json::from_value::<FactoryRunSummary>(value).is_err(),
            "{field}"
        );
    }
}

#[test]
fn invalid_accounting_counters_fail() {
    for field in ["activeMs", "subagents", "nanoAiu"] {
        for invalid in [json!(-1), json!(1.5), Value::Null, json!("1")] {
            let mut value = summary();
            value["consumed"][field] = invalid;
            assert!(
                serde_json::from_value::<FactoryRunSummary>(value).is_err(),
                "{field}"
            );
        }
    }
}

#[test]
fn summary_resume_flag_and_terminal_details_are_retained() {
    let mut value = summary();
    value["status"] = json!("error");
    value["canResume"] = json!(false);
    value["terminal"] = json!({"reason": "interrupted", "failure": {"code": "storage"}});
    let decoded: FactoryRunSummary = serde_json::from_value(value).expect("summary");
    assert_eq!(
        (decoded.can_resume, decoded.terminal),
        (
            Some(false),
            Some(json!({"reason": "interrupted", "failure": {"code": "storage"}}))
        )
    );
}

#[test]
fn invalid_summary_resume_flags_fail() {
    for can_resume in [Value::Null, json!(1), json!("true")] {
        let mut value = summary();
        value["canResume"] = can_resume;
        assert!(serde_json::from_value::<FactoryRunSummary>(value).is_err());
    }
}

#[test]
fn unknown_response_fields_are_accepted() {
    let mut value = envelope("running");
    value["futureField"] = json!({"v": 1});
    let mut accounting = summary();
    accounting["futureField"] = json!(true);
    let accounting: FactoryRunSummary = serde_json::from_value(accounting).expect("summary");
    assert_eq!(
        (decode(value).status, accounting.status),
        (FactoryRunStatus::Running, FactoryRunStatus::Running)
    );
}

#[test]
fn resume_wrapper_preserves_factory_and_run_identity() {
    let resumed: FactoryResumeResult = serde_json::from_value(json!({
        "factoryName": "verify", "run": envelope("running"), "futureField": true,
    }))
    .expect("resume");
    assert_eq!(
        (resumed.factory_name.as_str(), resumed.run.run_id.as_str()),
        ("verify", "run-1")
    );
}

#[test]
fn missing_resume_identity_fails() {
    for value in [
        json!({"run": envelope("running")}),
        json!({"factoryName": "verify"}),
        json!({"factoryName": " ", "run": envelope("running")}),
        json!({"factoryName": "verify", "run": {"status": "running"}}),
    ] {
        assert!(serde_json::from_value::<FactoryResumeResult>(value).is_err());
    }
}
