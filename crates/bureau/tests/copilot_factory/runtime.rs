use std::path::PathBuf;

use bureau::config::{CopilotFactoryProfile, CopilotFactoryRuntime};
use serde_json::{Value, json};

use super::{UNSAFE_RELATIVE_PATHS, agent, factory, factory_value};

const PROFILE: &str = "copilot-sdk-factory-v1";
const REQUIRED_FIELDS: [&str; 6] = [
    "profile",
    "directory",
    "digest",
    "version",
    "executable",
    "dist",
];

fn runtime_value() -> Value {
    factory_value()["runtime"].clone()
}

fn rejects_field(field: &str, value: Value) -> bool {
    let mut encoded = runtime_value();
    encoded[field] = value;
    serde_json::from_value::<CopilotFactoryRuntime>(encoded).is_err()
}

#[test]
fn profile_is_exactly_the_bureau_sdk_capability_contract() {
    let runtime = factory("").into_inner().runtime;
    let encoded = serde_json::to_value(&runtime).expect("runtime JSON");
    assert_eq!(
        (runtime.profile, &encoded["profile"]),
        (CopilotFactoryProfile::SdkFactoryV1, &json!(PROFILE))
    );
}

#[test]
fn profile_rejects_release_guesses_aliases_and_other_contracts() {
    for value in [
        "SdkFactoryV1",
        "COPILOT-SDK-FACTORY-V1",
        "copilot_sdk_factory_v1",
        "copilot-sdk-factory-v2",
        "copilot-sdk-factory-v1 ",
        "protocol-3",
        "1.0.0",
        "",
        "0000000000000000000000000000000000000000",
    ] {
        assert!(rejects_field("profile", json!(value)), "{value}");
    }
}

#[test]
fn profile_requires_a_string_not_a_tagged_object_or_sequence() {
    for value in [
        Value::Null,
        json!(42),
        json!([PROFILE]),
        json!({(PROFILE): null}),
    ] {
        assert!(rejects_field("profile", value));
    }
}

#[test]
fn runtime_rejects_null_sequences_and_missing_objects() {
    let runtime = runtime_value();
    let sequence: Vec<Value> = [
        "profile",
        "directory",
        "digest",
        "version",
        "executable",
        "cli",
        "dist",
    ]
    .map(|field| runtime[field].clone())
    .into();
    for value in [
        Value::Null,
        json!([]),
        json!(sequence),
        json!("runtime"),
        json!({}),
    ] {
        assert!(serde_json::from_value::<CopilotFactoryRuntime>(value).is_err());
    }
}

#[test]
fn every_runtime_identity_field_is_required() {
    for field in REQUIRED_FIELDS {
        let mut encoded = runtime_value();
        encoded
            .as_object_mut()
            .expect("runtime object")
            .remove(field);
        let error = serde_json::from_value::<CopilotFactoryRuntime>(encoded)
            .expect_err("required runtime field");
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn required_runtime_identity_fields_are_not_nullable() {
    for field in REQUIRED_FIELDS {
        assert!(rejects_field(field, Value::Null), "{field}");
    }
}

#[test]
fn runtime_has_no_arbitrary_argv_env_or_unknown_fields() {
    for field in ["argv", "env", "arguments", "release", "runtime_flags"] {
        let mut encoded = runtime_value();
        encoded[field] = json!({});
        let error = serde_json::from_value::<CopilotFactoryRuntime>(encoded)
            .expect_err("unknown runtime field");
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn omitted_and_explicit_null_cli_are_equivalent() {
    let omitted = factory("").into_inner().runtime;
    let mut encoded = runtime_value();
    encoded["cli"] = Value::Null;
    let explicit: CopilotFactoryRuntime = serde_json::from_value(encoded).expect("null CLI");
    let serialized = serde_json::to_value(&explicit).expect("CLI serialization");
    assert_eq!((explicit, serialized.get("cli")), (omitted, None));
}

#[test]
fn runtime_directory_must_be_absolute_and_not_traverse_parents() {
    for path in [
        "",
        "runtime",
        "./runtime",
        "../runtime",
        "/runtime/../other",
        "/runtime/\0bad",
    ] {
        let mut selection = factory("");
        selection.runtime.directory = PathBuf::from(path);
        let found = agent(Some(selection)).field_errors();
        assert!(
            found[0].contains("copilot_factory.runtime.directory"),
            "{path}: {found:?}"
        );
    }
}

#[test]
fn runtime_paths_must_remain_inside_the_qualified_tree() {
    for path in UNSAFE_RELATIVE_PATHS {
        let mut selection = factory("");
        selection.runtime.executable = PathBuf::from(path);
        selection.runtime.cli = Some(PathBuf::from(path));
        selection.runtime.dist = PathBuf::from(path);
        let found = agent(Some(selection)).field_errors();
        assert_eq!(found.len(), 3, "{path}: {found:?}");
    }
}

#[test]
fn only_the_dist_directory_may_name_the_runtime_root() {
    let mut selection = factory("");
    selection.runtime.executable = PathBuf::from(".");
    selection.runtime.cli = Some(PathBuf::from("."));
    selection.runtime.dist = PathBuf::from(".");
    let found = agent(Some(selection)).field_errors();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|error| !error.contains("runtime.dist")));
}

#[test]
fn runtime_supports_relative_cli_entrypoints_and_root_or_nested_dist() {
    for dist in [".", "dist", "./dist/subdirectory"] {
        let mut selection = factory("");
        selection.runtime.executable = PathBuf::from("bin/node");
        selection.runtime.cli = Some(PathBuf::from("entrypoints/copilot.mjs"));
        selection.runtime.dist = PathBuf::from(dist);
        let encoded = serde_json::to_value(&selection).expect("factory JSON");
        let found = agent(Some(selection)).field_errors();
        assert_eq!(
            (&encoded["runtime"]["dist"], found),
            (&json!(dist), Vec::<String>::new())
        );
    }
}

#[test]
fn connect_version_rejects_blank_values() {
    for version in ["", " ", "\n\t"] {
        let mut selection = factory("");
        selection.runtime.version = version.to_owned();
        let found = agent(Some(selection)).field_errors();
        assert!(
            found[0].contains("copilot_factory.runtime.version"),
            "{found:?}"
        );
    }
}

#[test]
fn connect_version_is_not_inferred_or_normalized() {
    let mut selection = factory("");
    selection.runtime.version = "exact qualified build 42 ".to_owned();
    let encoded = serde_json::to_value(&selection).expect("exact version");
    assert!(agent(Some(selection)).field_errors().is_empty());
    assert_eq!(encoded["runtime"]["version"], "exact qualified build 42 ");
}
