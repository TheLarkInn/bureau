use std::path::PathBuf;

use bureau::config::{AdapterKind, CopilotFactory};
use serde_json::{Value, json};

use super::{UNSAFE_RELATIVE_PATHS, agent, config, errors, factory, factory_value};

const ARTIFACT_ERRORS: [&str; 8] = [
    "copilot_factory.extension_digest",
    "copilot_factory.metadata",
    "copilot_factory.runtime.directory",
    "copilot_factory.runtime.digest",
    "copilot_factory.runtime.version",
    "copilot_factory.runtime.executable",
    "copilot_factory.runtime.cli",
    "copilot_factory.runtime.dist",
];

fn invalid_digests() -> Vec<String> {
    let prefix = "tree-sha256:";
    vec![
        String::new(),
        format!("{prefix}{}", "0".repeat(63)),
        format!("{prefix}{}", "0".repeat(65)),
        format!("{prefix}{}", "A".repeat(64)),
        format!("{prefix}{}", "g".repeat(64)),
        format!("sha256:{}", "0".repeat(64)),
        format!("TREE-SHA256:{}", "0".repeat(64)),
        format!("{prefix}{} ", "0".repeat(64)),
    ]
}

#[test]
fn extension_digest_and_runtime_are_required() {
    for field in ["extension_digest", "runtime"] {
        let mut encoded = factory_value();
        encoded
            .as_object_mut()
            .expect("factory object")
            .remove(field);
        let error = serde_json::from_value::<CopilotFactory>(encoded).expect_err("required field");
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn required_artifact_fields_reject_null_and_malformed_types() {
    let cases = [
        ("extension_digest", Value::Null),
        ("extension_digest", json!(42)),
        ("metadata", Value::Null),
        ("metadata", json!(["factory.json"])),
        ("runtime", Value::Null),
        ("runtime", json!([])),
    ];
    for (field, value) in cases {
        let mut encoded = factory_value();
        encoded[field] = value;
        assert!(
            serde_json::from_value::<CopilotFactory>(encoded).is_err(),
            "{field}"
        );
    }
}

#[test]
fn both_tree_digests_use_canonical_lowercase_sha256() {
    for digest in invalid_digests() {
        let mut selection = factory("");
        selection.extension_digest.clone_from(&digest);
        selection.runtime.digest = digest;
        let found = agent(Some(selection)).field_errors();
        let fields = [
            "copilot_factory.extension_digest",
            "copilot_factory.runtime.digest",
        ];
        let reported = fields.map(|field| found.iter().any(|error| error.contains(field)));
        assert_eq!((found.len(), reported), (2, [true; 2]), "{found:?}");
    }
}

#[test]
fn digest_validation_checks_syntax_not_artifact_contents() {
    for hex in ["0".repeat(64), "f".repeat(64), "0123456789abcdef".repeat(4)] {
        let mut selection = factory("");
        let fields = &mut *selection;
        fields.extension_digest = format!("tree-sha256:{hex}");
        fields.runtime.digest.clone_from(&fields.extension_digest);
        let found = agent(Some(selection)).field_errors();
        assert!(found.is_empty(), "{found:?}");
    }
}

#[test]
fn metadata_defaults_to_the_standard_factory_json_file() {
    let selection = factory("");
    let encoded = serde_json::to_value(&selection).expect("serialize metadata");
    assert_eq!(
        (selection.into_inner().metadata, &encoded["metadata"]),
        (PathBuf::from("factory.json"), &json!("factory.json"))
    );
}

#[test]
fn metadata_accepts_safe_relative_file_paths_without_reading_them() {
    for path in [
        "factory.json",
        "metadata/factory.json",
        "./metadata/./factory meta.json",
    ] {
        let mut selection = factory("");
        selection.metadata = PathBuf::from(path);
        let encoded = serde_json::to_value(&selection).expect("metadata JSON");
        let found = agent(Some(selection)).field_errors();
        assert_eq!(
            (&encoded["metadata"], found),
            (&json!(path), Vec::<String>::new())
        );
    }
}

#[test]
fn metadata_cannot_escape_the_extension_or_name_its_root() {
    for path in UNSAFE_RELATIVE_PATHS.iter().copied().chain([".", "./"]) {
        let mut selection = factory("");
        selection.metadata = PathBuf::from(path);
        let found = agent(Some(selection)).field_errors();
        assert!(
            found[0].contains("copilot_factory.metadata"),
            "{path}: {found:?}"
        );
    }
}

#[test]
fn all_artifact_errors_accumulate_with_pipeline_context() {
    let mut selection = factory("");
    selection.extension_digest.clear();
    selection.metadata = PathBuf::from("../factory.json");
    selection.runtime.directory = PathBuf::from("relative");
    selection.runtime.digest.clear();
    selection.runtime.version.clear();
    selection.runtime.executable = PathBuf::from("../host");
    selection.runtime.cli = Some(PathBuf::from("../cli.js"));
    selection.runtime.dist = PathBuf::from("../dist");
    let found = errors(&config(vec![agent(Some(selection))], AdapterKind::Copilot));
    let reported = ARTIFACT_ERRORS.map(|field| found.iter().any(|error| error.contains(field)));
    assert_eq!((found.len(), reported), (8, [true; 8]), "{found:?}");
    assert!(
        found
            .iter()
            .all(|error| error.contains("pipeline `inspect` step `review`"))
    );
}
