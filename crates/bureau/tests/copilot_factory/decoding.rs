use bureau::config::{CopilotFactory, CopilotFactoryLimits, StepDef};
use serde_json::{Value, json};

use super::{agent, factory, factory_yaml};

const NESTED_ARGS: &str = r#"{
    "enabled": true,
    "integer": 9007199254740993,
    "real": 1.25,
    "nested": {"nothing": null, "choices": [false, 2, "three", {"value": true}]},
    "literal": "${inputs_from.prepare}"
}"#;

fn rejects_limit_values(field: &str, values: &[&str]) {
    for value in values {
        let yaml = format!("{field}: {value}");
        assert!(
            serde_yaml_ng::from_str::<CopilotFactoryLimits>(&yaml).is_err(),
            "{yaml}"
        );
    }
}

#[test]
fn nested_argument_types_and_limits_roundtrip() {
    let expected: Value = serde_json::from_str(NESTED_ARGS).expect("nested arguments");
    let original = factory(&format!(
        "args: {NESTED_ARGS}\nlimits: {{max_concurrent_subagents: 2, max_total_subagents: 7, timeout_seconds: 0.5, max_ai_credits: 3.25}}"
    ));
    let json = serde_json::to_string(&original).expect("encode JSON");
    let yaml = serde_yaml_ng::to_string(&original).expect("encode YAML");
    let decoded = [
        serde_json::from_str::<CopilotFactory>(&json).expect("decode JSON"),
        serde_yaml_ng::from_str::<CopilotFactory>(&yaml).expect("decode YAML"),
    ];
    assert_eq!(original.args, expected);
    assert_eq!(decoded, [original.clone(), original]);
}

#[test]
fn omitted_and_explicit_null_args_are_equivalent() {
    let omitted = factory("");
    let explicit = factory("args: null");
    let serialized = serde_json::to_value(&omitted).expect("serialize default args");
    assert_eq!(
        (omitted, &serialized["args"], serialized.get("limits")),
        (explicit, &Value::Null, None)
    );
}

#[test]
fn omitted_limit_fields_have_no_overrides() {
    let limits: CopilotFactoryLimits = serde_yaml_ng::from_str("{}").expect("empty limits");
    let encoded = serde_json::to_value(&limits).expect("encode limits");
    assert_eq!(
        (limits, encoded),
        (CopilotFactoryLimits::default(), json!({}))
    );
}

#[test]
fn limit_fields_are_independently_optional() {
    let cases = [
        ("max_concurrent_subagents", json!(2)),
        ("max_total_subagents", json!(7)),
        ("timeout_seconds", json!(0.5)),
        ("max_ai_credits", json!(3.25)),
    ];
    for (field, value) in cases {
        let encoded = json!({(field): value});
        let limits: CopilotFactoryLimits =
            serde_json::from_value(encoded.clone()).expect("one override");
        assert_eq!(serde_json::to_value(limits).expect("encode"), encoded);
    }
}

#[test]
fn the_step_export_preserves_factory_selection() {
    let expected = agent(Some(factory("args: {value: [1, null, true]}")));
    let encoded = serde_yaml_ng::to_string(&expected).expect("encode step");
    let actual: StepDef = serde_yaml_ng::from_str(&encoded).expect("decode step");
    assert_eq!(actual, expected);
}

#[test]
fn unknown_factory_and_limit_fields_are_rejected() {
    let cases = [
        ("unexpected: true", "unexpected"),
        ("resume_from_run_id: previous", "resume_from_run_id"),
        (
            "limits: {maxConcurrentSubagents: 2}",
            "maxConcurrentSubagents",
        ),
        ("limits: {max_cost_per_day_usd: 1}", "max_cost_per_day_usd"),
        ("limits: {retry_ceiling: 2}", "retry_ceiling"),
    ];
    for (fields, expected) in cases {
        let yaml = factory_yaml(fields);
        let error = serde_yaml_ng::from_str::<CopilotFactory>(&yaml).expect_err("strict fields");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn the_optional_limits_block_rejects_null_and_nonobjects() {
    for value in ["null", "[]", "true", "'{}'"] {
        let yaml = factory_yaml(&format!("limits: {value}"));
        assert!(
            serde_yaml_ng::from_str::<CopilotFactory>(&yaml).is_err(),
            "{yaml}"
        );
    }
}

#[test]
fn malformed_factory_shapes_are_rejected() {
    let cases = [
        "null",
        "[]",
        r#"["review","project:review"]"#,
        "true",
        "{}",
        r#"{"name":"review"}"#,
        r#"{"extension":"project:review"}"#,
        r#"{"name":42,"extension":"project:review"}"#,
        r#"{"name":"review","extension":[]}"#,
        r#"{"name":"review","extension":"project:review","limits":[]}"#,
    ];
    for text in cases {
        assert!(
            serde_json::from_str::<CopilotFactory>(text).is_err(),
            "{text}"
        );
    }
}

#[test]
fn limit_overrides_require_an_object() {
    for text in ["[]", "[1,2,3,4]", "null", "true", "\"{}\""] {
        assert!(
            serde_json::from_str::<CopilotFactoryLimits>(text).is_err(),
            "{text}"
        );
    }
}

#[test]
fn integer_limit_types_are_strict_and_null_is_not_unlimited() {
    for field in ["max_concurrent_subagents", "max_total_subagents"] {
        rejects_limit_values(
            field,
            &["-1", "1.5", "4294967296", "'1'", "true", "[]", "{}", "null"],
        );
    }
}

#[test]
fn numeric_limit_types_are_strict_and_null_is_not_unlimited() {
    for field in ["timeout_seconds", "max_ai_credits"] {
        rejects_limit_values(field, &["'1'", "true", "[]", "{}", "null"]);
    }
}
