//! Layer 2 step I/O contract tests (DESIGN.md section 7): serde
//! round-trips, schema versioning, outcome and trust semantics.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bureau::contract::{
    Artifact, DecodeError, SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust, WorkItem,
};

fn sample_request() -> StepRequest {
    StepRequest {
        schema: SCHEMA_VERSION.to_owned(),
        run_id: "run-1".to_owned(),
        step: "propose".to_owned(),
        worktree: PathBuf::from("/tmp/wt"),
        item: WorkItem {
            external_id: "acme/web#42".to_owned(),
            title: "Fix the flaky login test".to_owned(),
            body: "Fails intermittently on CI.".to_owned(),
            url: "https://example.invalid/acme/web/issues/42".to_owned(),
            labels: vec!["bug".to_owned()],
        },
        trust: Trust::Maintainer,
        inputs: BTreeMap::from([("test_output".to_owned(), serde_json::json!("FAIL"))]),
        artifacts: BTreeMap::from([("diff".to_owned(), PathBuf::from("artifacts/diff.patch"))]),
    }
}

fn sample_result() -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::NoWork,
        outputs: BTreeMap::from([("verdict".to_owned(), serde_json::json!("pass"))]),
        artifacts: vec![Artifact {
            name: "diff".to_owned(),
            path: PathBuf::from("artifacts/diff.patch"),
        }],
        trust: Trust::Derived,
        message: String::new(),
    }
}

#[test]
fn request_round_trips() {
    let request = sample_request();
    let bytes = request.to_json().expect("serialize");
    assert_eq!(StepRequest::from_json(&bytes).expect("parse"), request);
}

#[test]
fn result_round_trips() {
    let result = sample_result();
    let bytes = result.to_json().expect("serialize");
    assert_eq!(StepResult::from_json(&bytes).expect("parse"), result);
}

#[test]
fn wrong_schema_is_rejected_with_the_received_value() {
    let bytes = br#"{"schema":"v7","run_id":"","step":"","worktree":".","trust":"trusted","inputs":{},"artifacts":{}}"#;
    let error = StepRequest::from_json(bytes).expect_err("v7 must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("\"v7\""),
        "message names the received value: {message}"
    );
    assert!(
        message.contains(SCHEMA_VERSION),
        "message names the expected value: {message}"
    );
}

#[test]
fn missing_schema_is_rejected() {
    let error = StepResult::from_json(b"{}").expect_err("schema is required");
    assert!(matches!(error, DecodeError::Schema { .. }), "{error:?}");
}

#[test]
fn malformed_json_is_a_decode_error() {
    let error = StepRequest::from_json(b"not json").expect_err("must fail");
    assert!(matches!(error, DecodeError::Json(_)), "{error:?}");
}

#[test]
fn outcome_wire_form_is_kebab_case() {
    let bytes = sample_result().to_json().expect("serialize");
    let text = String::from_utf8(bytes).expect("utf8");
    assert!(
        text.contains("\"outcome\":\"no-work\""),
        "wire form: {text}"
    );
    assert_eq!(
        serde_json::from_str::<StepOutcome>("\"blocked\"").expect("parse"),
        StepOutcome::Blocked
    );
}

#[test]
fn only_failure_consumes_retry_budget() {
    let cases = [
        (StepOutcome::Success, false),
        (StepOutcome::Failure, true),
        (StepOutcome::Blocked, false),
        (StepOutcome::NoWork, false),
    ];
    for (outcome, expected) in cases {
        assert_eq!(outcome.consumes_retry(), expected, "{outcome:?}");
    }
}

#[test]
fn trust_orders_by_declaration() {
    assert!(Trust::Untrusted < Trust::Derived);
    assert!(Trust::Derived < Trust::Maintainer);
    assert!(Trust::Maintainer < Trust::Trusted);
}

#[test]
fn trust_wire_form_is_snake_case() {
    assert_eq!(
        serde_json::from_str::<Trust>("\"maintainer\"").expect("parse"),
        Trust::Maintainer
    );
    assert_eq!(
        serde_json::to_string(&Trust::Trusted).expect("serialize"),
        "\"trusted\""
    );
}

#[test]
fn a_request_carries_the_work_item() {
    let text = String::from_utf8(sample_request().to_json().expect("serialize")).expect("utf8");
    let carried = [
        "acme/web#42",
        "Fix the flaky login test",
        "Fails intermittently on CI.",
        "https://example.invalid/acme/web/issues/42",
    ];
    assert!(
        carried.iter().all(|part| text.contains(part)),
        "wire form: {text}"
    );
}

#[test]
fn a_request_without_an_item_still_parses() {
    let bytes = br#"{"schema":"v2","run_id":"r","step":"s","worktree":".","trust":"trusted","inputs":{},"artifacts":{}}"#;
    let request = StepRequest::from_json(bytes).expect("item is optional on the wire");
    assert_eq!(request.item, WorkItem::default());
}
