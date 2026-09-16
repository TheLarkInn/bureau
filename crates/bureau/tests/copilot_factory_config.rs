//! Offline schema and validation coverage for explicit local Copilot factories.

#[path = "copilot_factory/artifacts.rs"]
mod artifacts;
#[path = "copilot_factory/credentials.rs"]
mod credentials;
#[path = "copilot_factory/decoding.rs"]
mod decoding;
#[path = "copilot_factory/fields.rs"]
mod fields;
#[path = "copilot_factory/pipeline.rs"]
mod pipeline;
#[path = "copilot_factory/runtime.rs"]
mod runtime;

use std::collections::BTreeMap;

use bureau::config::{AdapterKind, Config, CopilotFactory, Pipeline, Role, StepDef, validate};

const FACTORY_YAML: &str = r"
name: review
extension: project:review
extension_digest: tree-sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
model_credential: copilot-model
runtime:
  profile: copilot-sdk-factory-v1
  directory: /preprovisioned/copilot-sdk
  digest: tree-sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  version: expected-connect-version
  executable: bin/copilot
  dist: dist
";

const UNSAFE_RELATIVE_PATHS: &[&str] = &[
    "",
    " ",
    "..",
    "../file",
    "nested/../file",
    "/absolute/file",
    "//host/share/file",
    "\\rooted\\file",
    "\\\\host\\share\\file",
    "C:\\absolute\\file",
    "C:/absolute/file",
    "C:relative",
    "..\\file",
    "nested\\..\\file",
    "file\0name",
];

fn factory_yaml(fields: &str) -> String {
    format!("{FACTORY_YAML}\n{fields}")
}

fn factory(fields: &str) -> CopilotFactory {
    serde_yaml_ng::from_str(&factory_yaml(fields)).expect("factory schema")
}

fn factory_value() -> serde_json::Value {
    serde_json::to_value(factory("")).expect("factory JSON")
}

fn agent(factory: Option<CopilotFactory>) -> StepDef {
    let mut step: StepDef =
        serde_yaml_ng::from_str("name: review\ntype: agent\nrole: reviewer\nnext: done")
            .expect("agent step");
    step.copilot_factory = factory;
    step
}

fn config(steps: Vec<StepDef>, adapter: AdapterKind) -> Config {
    let mut role: Role = serde_yaml_ng::from_str(
        "name: reviewer\nagent: /bureau:reviewer\nadapter: copilot\npermissions: [repo:read, model:invoke]\nmin_trust: untrusted",
    )
    .expect("role");
    role.adapter = adapter;
    let pipeline = Pipeline {
        name: "inspect".to_owned(),
        steps,
    };
    Config {
        repos: BTreeMap::new(),
        roles: BTreeMap::from([(role.name.clone(), role)]),
        assignments: BTreeMap::new(),
        label_rules: BTreeMap::new(),
        pipelines: BTreeMap::from([(pipeline.name.clone(), pipeline)]),
    }
}

fn errors(config: &Config) -> Vec<String> {
    validate(config)
        .into_iter()
        .map(|error| error.to_string())
        .collect()
}
