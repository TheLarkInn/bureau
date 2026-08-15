//! Step-contract adversarial edges (DESIGN.md layer 2): unknown fields,
//! a non-string schema, and out-of-range costs.

use std::collections::BTreeMap;

use bureau::contract::{DecodeError, SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust};

fn result_wire(cost: &str) -> Vec<u8> {
    let schema = SCHEMA_VERSION;
    format!(
        r#"{{"schema":"{schema}","outcome":"success","outputs":{{}},"artifacts":[],"trust":"derived","cost_usd":{cost},"message":""}}"#
    )
    .into_bytes()
}

#[test]
fn unknown_fields_are_ignored_by_the_contract() {
    // serde's default is to ignore unknown fields. Unlike the config
    // files (deny_unknown_fields), that is right for a versioned wire
    // contract: `schema` gates breaking changes, so a newer step
    // emitting an extra field must not break an older runner.
    let request = br#"{"schema":"v1","run_id":"r","step":"s","worktree":".","trust":"derived","inputs":{},"artifacts":{},"surprise":true}"#;
    let parsed = StepRequest::from_json(request).expect("unknown fields parse");
    assert_eq!(parsed.run_id, "r");
    let result = br#"{"schema":"v1","outcome":"success","outputs":{},"artifacts":[],"trust":"derived","cost_usd":0.1,"message":"","extra":{"nested":1}}"#;
    let parsed = StepResult::from_json(result).expect("unknown fields parse");
    assert_eq!(parsed.cost_usd.to_bits(), 0.1f64.to_bits());
}

#[test]
fn a_non_string_schema_names_the_received_value() {
    // DESIGN.md layer 2 requires the RECEIVED value in the error, so a
    // non-string `schema` (here the number 1) renders as its JSON form;
    // `<missing>` is reserved for an absent field.
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
fn a_nan_cost_cannot_survive_the_wire() {
    let result = StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Success,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        cost_usd: f64::NAN,
        message: String::new(),
    };
    let wire = result.to_json().expect("serialize");
    // serde_json renders non-finite floats as null, and null then fails
    // the f64 decode — a NaN cost is ejected at the boundary instead of
    // reaching the budget sums, where `NaN >= limit` is false and would
    // read as infinite headroom.
    let text = String::from_utf8(wire).expect("utf8");
    assert!(text.contains(r#""cost_usd":null"#), "{text}");
    assert!(StepResult::from_json(text.as_bytes()).is_err());
    assert!(StepResult::from_json(&result_wire("NaN")).is_err());
}

#[test]
fn a_negative_cost_is_accepted_by_the_contract() {
    // The contract is a transport: any finite f64 decodes. DESIGN.md
    // layer 2 puts no range on cost_usd; enforcement is layer 5's job
    // (headroom), so a negative cost would credit the day's budget.
    let result = StepResult::from_json(&result_wire("-3.5")).expect("parses");
    assert_eq!(result.cost_usd.to_bits(), (-3.5f64).to_bits());
}
