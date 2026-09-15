use serde_json::{Value, json};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

use super::validate;

fn nested_schema() -> Value {
    json!({
        "type": "object", "required": ["items"], "additionalProperties": false,
        "properties": {"items": {
            "type": "array", "minItems": 1, "uniqueItems": true,
            "items": {"type": "object", "required": ["name"], "additionalProperties": false,
                "properties": {"name": {"type": "string", "pattern": "^[a-z]+$", "minLength": 2}}}
        }}
    })
}

fn composition_schema() -> Value {
    json!({
        "type": "object", "required": ["count"],
        "allOf": [
            {"properties": {"count": {"type": "integer", "minimum": 1, "maximum": 10}}},
            {"properties": {"count": {"multipleOf": 2}}}
        ],
        "if": {"properties": {"count": {"minimum": 6}}},
        "then": {"required": ["approved"], "properties": {"approved": {"const": true}}},
        "not": {"required": ["forbidden"]}
    })
}

fn reference_schema() -> Value {
    json!({
        "definitions": {"label": {"type": "string", "minLength": 2, "enum": ["ok", "yes"]}},
        "type": "object", "required": ["label"],
        "properties": {"label": {"$ref": "#/definitions/label"}}
    })
}

fn dynamic_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": {"node": {
            "$dynamicAnchor": "node", "type": "object", "required": ["value"],
            "properties": {"value": {"type": "integer"}}
        }},
        "$dynamicRef": "#node"
    })
}

#[test]
fn validates_nested_constraints_instead_of_only_runtime_subset_keywords() {
    let cases = [
        (json!({"items": [{"name": "ok"}]}), true),
        (json!({"items": []}), false),
        (json!({"items": [{"name": "a"}]}), false),
        (json!({"items": [{"name": "UPPER"}]}), false),
        (json!({"items": [{"name": "ok"}, {"name": "ok"}]}), false),
        (json!({"items": [{"name": "ok", "extra": true}]}), false),
    ];
    for (arguments, valid) in cases {
        assert_eq!(validate(&nested_schema(), &arguments).is_ok(), valid);
    }
}

#[test]
fn validates_all_compositions_conditions_and_numeric_constraints() {
    let cases = [
        (json!({"count": 4}), true),
        (json!({"count": 8, "approved": true}), true),
        (json!({"count": 3}), false),
        (json!({"count": 8}), false),
        (json!({"count": 8, "approved": false}), false),
        (json!({"count": 12, "approved": true}), false),
        (json!({"count": 2, "forbidden": null}), false),
    ];
    for (arguments, valid) in cases {
        assert_eq!(validate(&composition_schema(), &arguments).is_ok(), valid);
    }
}

#[test]
fn internal_references_enforce_the_referenced_constraints() {
    let cases = [
        (json!({"label": "ok"}), true),
        (json!({"label": "no"}), false),
    ];
    for (arguments, valid) in cases {
        assert_eq!(validate(&reference_schema(), &arguments).is_ok(), valid);
    }
}

#[test]
fn internal_dynamic_references_enforce_their_target() {
    let cases = [
        (json!({"value": 1}), true),
        (json!({"value": "one"}), false),
    ];
    for (arguments, valid) in cases {
        assert_eq!(validate(&dynamic_schema(), &arguments).is_ok(), valid);
    }
}

#[test]
fn any_of_and_one_of_are_not_treated_as_annotations() {
    let cases = [
        (
            json!({"anyOf": [{"const": 1}, {"const": 2}]}),
            json!(2),
            true,
        ),
        (
            json!({"anyOf": [{"const": 1}, {"const": 2}]}),
            json!(3),
            false,
        ),
        (
            json!({"oneOf": [{"type": "number"}, {"type": "integer"}]}),
            json!(1),
            false,
        ),
    ];
    for (schema, arguments, valid) in cases {
        assert_eq!(validate(&schema, &arguments).is_ok(), valid);
    }
}

#[test]
fn malformed_schemas_and_unknown_dialects_fail_closed() {
    let schemas = [
        Value::Null,
        json!([]),
        json!({"$schema": "https://example.invalid/unknown-draft"}),
        json!({"$schema": 7}),
        json!({"type": "imaginary"}),
        json!({"$ref": "#/definitions/missing"}),
    ];
    for schema in schemas {
        assert!(validate(&schema, &json!({})).is_err());
    }
}

#[test]
fn boolean_schemas_have_their_actual_meaning() {
    assert_eq!(
        (
            validate(&json!(true), &Value::Null).is_ok(),
            validate(&json!(false), &Value::Null).is_ok()
        ),
        (true, false)
    );
}

#[test]
fn external_url_references_are_rejected_without_a_fetcher() {
    let schema = json!({"$ref": "https://example.invalid/external.json"});
    let error = validate(&schema, &json!({})).expect_err("external reference");
    assert!(
        error
            .to_string()
            .contains("external factory schema resources are forbidden")
    );
}

struct SchemaFile(std::path::PathBuf);

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

impl SchemaFile {
    fn new() -> Self {
        let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let name = format!("bureau-factory-schema-{}-{id}.json", std::process::id());
        let path = std::env::temp_dir().join(name);
        let value = json!({"$dynamicAnchor": "node", "type": "object"});
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("new schema fixture");
        file.write_all(value.to_string().as_bytes())
            .expect("schema bytes");
        Self(path)
    }

    fn url(&self) -> String {
        format!("file://{}", self.0.display())
    }
}

impl Drop for SchemaFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).expect("remove fixture");
    }
}

#[test]
fn even_existing_external_files_cannot_be_loaded() {
    let file = SchemaFile::new();
    let schema = json!({"$ref": file.url()});
    let error = validate(&schema, &json!({})).expect_err("external file");
    assert!(
        error
            .to_string()
            .contains("external factory schema resources are forbidden")
    );
}

#[test]
fn external_dynamic_references_cannot_load_existing_files() {
    let file = SchemaFile::new();
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$dynamicRef": format!("{}#node", file.url())
    });
    let error = validate(&schema, &json!({})).expect_err("external dynamic reference");
    assert!(
        error
            .to_string()
            .contains("external factory schema resources are forbidden")
    );
}
