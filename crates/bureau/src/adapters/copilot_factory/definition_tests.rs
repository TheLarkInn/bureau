use serde_json::{Value, json};

use super::Definition;

fn metadata() -> Value {
    json!({
        "name": "review", "description": "Review a change", "phases": [{"title": "Review"}],
        "argsSchema": {
            "type": "object", "required": ["count"],
            "properties": {"count": {"type": "integer", "minimum": 2, "multipleOf": 2}},
            "additionalProperties": false
        },
        "limits": {"maxConcurrentSubagents": 2, "maxAiCredits": 3.5}
    })
}

fn parse(value: &Value, arguments: &Value) -> Result<Definition, String> {
    let bytes = serde_json::to_vec(value).expect("metadata JSON");
    Definition::parse(&bytes, "review", arguments).map_err(String::from)
}

#[test]
fn preserves_sdk_metadata_and_raw_limit_values() {
    let definition = parse(&metadata(), &json!({"count": 4})).expect("valid definition");
    assert_eq!(
        serde_json::to_value(definition).expect("metadata"),
        metadata()
    );
}

#[test]
fn enforces_argument_constraints_before_initialization() {
    let cases = [
        (json!({"count": 2}), true),
        (json!({"count": 1}), false),
        (json!({"count": 3}), false),
        (json!({"count": 2, "extra": true}), false),
        (Value::Null, false),
    ];
    for (arguments, valid) in cases {
        assert_eq!(parse(&metadata(), &arguments).is_ok(), valid);
    }
}

#[test]
fn metadata_name_must_match_the_approved_selection() {
    let bytes = serde_json::to_vec(&metadata()).expect("metadata JSON");
    assert!(Definition::parse(&bytes, "different", &json!({"count": 2})).is_err());
}

#[test]
fn malformed_native_limit_values_fail_closed() {
    let cases = [
        json!({"maxConcurrentSubagents": 0}),
        json!({"maxConcurrentSubagents": 501}),
        json!({"maxTotalSubagents": 1.5}),
        json!({"timeoutSeconds": 0}),
        json!({"maxAiCredits": null}),
        json!({"maxAiCredits": -1}),
        json!({"maxAiCredits": "3"}),
        json!({"invented": 1}),
    ];
    for limits in cases {
        let mut value = metadata();
        value["limits"] = limits;
        assert!(parse(&value, &json!({"count": 2})).is_err());
    }
}

#[test]
fn metadata_does_not_invent_an_unverified_total_or_timeout_maximum() {
    let mut value = metadata();
    value["limits"] = json!({"maxTotalSubagents": 4_294_967_295_u64, "timeoutSeconds": 3_000_000});
    assert!(parse(&value, &json!({"count": 2})).is_ok());
}

#[test]
fn metadata_and_phase_objects_cannot_be_positional_arrays() {
    let cases = [
        json!(["review", "Review", []]),
        json!({"name": "review", "description": "Review", "phases": [["Review"]]}),
    ];
    for value in cases {
        assert!(parse(&value, &json!({})).is_err());
    }
}

#[test]
fn unknown_metadata_fields_are_not_ignored() {
    let mut value = metadata();
    value["environment"] = json!({"GH_TOKEN": "not-a-grant"});
    assert!(parse(&value, &json!({"count": 2})).is_err());
}

#[test]
fn duplicate_metadata_fields_are_rejected() {
    let bytes = br#"{"name":"other","name":"review","description":"","phases":[]}"#;
    assert!(Definition::parse(bytes, "review", &Value::Null).is_err());
}

#[test]
fn absent_schema_is_distinct_from_invalid_null_schema() {
    let absent = json!({"name": "review", "description": "", "phases": []});
    let mut invalid = absent.clone();
    invalid["argsSchema"] = Value::Null;
    assert_eq!(
        (
            parse(&absent, &Value::Null).is_ok(),
            parse(&invalid, &Value::Null).is_ok()
        ),
        (true, false)
    );
}

#[test]
fn metadata_cannot_reference_external_schema_resources() {
    let mut value = metadata();
    value["argsSchema"] = json!({"$ref": "file:///unapproved/schema.json"});
    let error = parse(&value, &json!({})).expect_err("external schema");
    assert!(error.contains("external factory schema resources are forbidden"));
}
