//! Step-contract adversarial edges: unknown fields and schema cutover.

use bureau::contract::{DecodeError, SCHEMA_VERSION, StepRequest, StepResult};

#[test]
fn unknown_fields_are_ignored_by_the_contract() {
    let schema = SCHEMA_VERSION;
    let request = format!(
        r#"{{"schema":"{schema}","run_id":"r","step":"s","worktree":".","trust":"derived","inputs":{{}},"artifacts":{{}},"surprise":true}}"#
    );
    let parsed = StepRequest::from_json(request.as_bytes()).expect("request");
    assert_eq!(parsed.run_id, "r");
    let result = format!(
        r#"{{"schema":"{schema}","outcome":"success","outputs":{{}},"artifacts":[],"trust":"derived","cost_usd":999,"message":"ok","extra":{{"nested":1}}}}"#
    );
    let parsed = StepResult::from_json(result.as_bytes()).expect("result");
    assert_eq!(parsed.message, "ok");
}

#[test]
fn a_non_string_schema_names_the_received_value() {
    let bytes = br#"{"schema":1,"run_id":"r","step":"s","worktree":".","trust":"derived","inputs":{},"artifacts":{}}"#;
    let error = StepRequest::from_json(bytes).expect_err("schema 1 is rejected");
    assert!(matches!(&error, DecodeError::Schema { .. }), "{error:?}");
    let message = error.to_string();
    assert!(
        message.contains('1') && !message.contains("<missing>"),
        "{message}"
    );
}

#[test]
fn v1_result_is_rejected_after_the_cost_cutover() {
    let result = br#"{"schema":"v1","outcome":"success","outputs":{},"artifacts":[],"trust":"derived","cost_usd":0.1,"message":""}"#;
    let error = StepResult::from_json(result).expect_err("v1 must fail");
    assert!(error.to_string().contains(SCHEMA_VERSION), "{error}");
}
