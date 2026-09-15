use bureau::config::{AdapterKind, Config, CopilotFactory, Permission};
use serde_json::{Value, json};

use super::{agent, config, errors, factory, factory_value, factory_yaml};

fn without_model_grant(config: &mut Config) {
    config
        .roles
        .get_mut("reviewer")
        .expect("reviewer")
        .permissions
        .retain(|permission| *permission != Permission::ModelInvoke);
}

#[test]
fn model_credential_is_required_in_json_and_yaml() {
    let mut encoded = factory_value();
    encoded
        .as_object_mut()
        .expect("factory")
        .remove("model_credential");
    let yaml = factory_yaml("").replace("model_credential: copilot-model\n", "");
    let found = [
        serde_json::from_value::<CopilotFactory>(encoded)
            .expect_err("required")
            .to_string(),
        serde_yaml_ng::from_str::<CopilotFactory>(&yaml)
            .expect_err("required")
            .to_string(),
    ];
    assert!(
        found.iter().all(|error| error.contains("model_credential")),
        "{found:?}"
    );
}

#[test]
fn model_credential_must_be_a_string_not_null_or_a_value_object() {
    for value in [
        Value::Null,
        json!(42),
        json!(0.5),
        json!(true),
        json!([]),
        json!({}),
    ] {
        let mut encoded = factory_value();
        encoded["model_credential"] = value;
        assert!(serde_json::from_value::<CopilotFactory>(encoded).is_err());
    }
}

#[test]
fn credential_references_preserve_exact_names_through_both_formats() {
    for reference in ["copilot-model", "Copilot_Model-01", "model_2"] {
        let mut expected = factory("");
        expected.model_credential = reference.to_owned();
        let json = serde_json::to_string(&expected).expect("JSON");
        let yaml = serde_yaml_ng::to_string(&expected).expect("YAML");
        let decoded = [
            serde_json::from_str::<CopilotFactory>(&json).expect("JSON roundtrip"),
            serde_yaml_ng::from_str::<CopilotFactory>(&yaml).expect("YAML roundtrip"),
        ];
        assert_eq!(decoded, [expected.clone(), expected]);
    }
}

#[test]
fn malformed_credential_references_are_rejected() {
    let cases = [
        "",
        " ",
        " model",
        "model ",
        "model/ref",
        "model\\ref",
        ".",
        "..",
        "${TOKEN}",
        "env:TOKEN",
        "file:token",
        "model.name",
        "model\0ref",
    ];
    for reference in cases {
        let mut selection = factory("");
        selection.model_credential = reference.to_owned();
        let found = agent(Some(selection)).field_errors();
        assert!(
            found[0].contains("copilot_factory.model_credential"),
            "{found:?}"
        );
    }
}

#[test]
fn config_validation_does_not_resolve_credentials_or_add_a_registry() {
    let mut selection = factory("");
    selection.model_credential = "declared-at-runtime".to_owned();
    let config = config(vec![agent(Some(selection))], AdapterKind::Copilot);
    let encoded = serde_json::to_value(&config).expect("config JSON");
    assert_eq!(
        (errors(&config), encoded.get("credentials")),
        (Vec::<String>::new(), None)
    );
}

#[test]
fn a_factory_role_must_authorize_model_invocation() {
    let mut config = config(vec![agent(Some(factory("")))], AdapterKind::Copilot);
    without_model_grant(&mut config);
    let found = errors(&config);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("`copilot_factory` requires `model:invoke` on its role"));
}

#[test]
fn ordinary_agents_do_not_acquire_a_new_config_permission_requirement() {
    let mut config = config(vec![agent(None)], AdapterKind::Copilot);
    without_model_grant(&mut config);
    assert!(errors(&config).is_empty());
}

#[test]
fn credential_role_and_adapter_errors_accumulate() {
    let mut selection = factory("");
    selection.model_credential.clear();
    let mut config = config(vec![agent(Some(selection))], AdapterKind::Claude);
    without_model_grant(&mut config);
    let found = errors(&config);
    let fields = [
        "copilot_factory.model_credential",
        "`copilot` adapter",
        "`model:invoke`",
    ];
    let reported = fields.map(|field| found.iter().any(|error| error.contains(field)));
    assert_eq!((found.len(), reported), (3, [true; 3]), "{found:?}");
}

#[test]
fn only_explicit_factory_steps_contribute_model_references() {
    let ordinary = agent(None);
    let mut selection = factory("");
    selection.model_credential = "copilot-model".to_owned();
    let config = config(vec![ordinary, agent(Some(selection))], AdapterKind::Copilot);
    let references: Vec<_> = config.pipelines["inspect"]
        .factory_credential_refs()
        .collect();
    assert_eq!(references, ["copilot-model"]);
}

#[test]
fn serialization_persists_only_the_declared_reference() {
    let encoded = factory_value();
    assert_eq!(encoded["model_credential"], json!("copilot-model"));
    assert!(encoded.get("credential_value").is_none());
}
